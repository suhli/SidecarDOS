#include <Windows.h>
#include <wdf.h>
#include <iddcx.h>
#include <d3d11_1.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <array>
#include <vector>
#include <thread>
#include <mutex>
#include <memory>
#include <algorithm>
#include "Shared.h"

using Microsoft::WRL::ComPtr;
namespace {
constexpr NTSTATUS Success=0;
constexpr NTSTATUS Invalid=static_cast<NTSTATUS>(0xc000000d);
constexpr NTSTATUS NotReady=static_cast<NTSTATUS>(0xc00000a3);
struct Device;
struct Context {Device* device;};
WDF_DECLARE_CONTEXT_TYPE(Context);
EVT_WDF_DRIVER_DEVICE_ADD AddDevice;
EVT_WDF_DEVICE_D0_ENTRY EnterD0;
EVT_WDF_DEVICE_D0_EXIT ExitD0;
EVT_IDD_CX_DEVICE_IO_CONTROL IoControl;
EVT_WDF_FILE_CLEANUP FileCleanup;
EVT_WDF_OBJECT_CONTEXT_CLEANUP Cleanup;
EVT_IDD_CX_ADAPTER_INIT_FINISHED AdapterReady;
EVT_IDD_CX_ADAPTER_COMMIT_MODES Commit;
EVT_IDD_CX_PARSE_MONITOR_DESCRIPTION Parse;
EVT_IDD_CX_MONITOR_GET_DEFAULT_DESCRIPTION_MODES DefaultModes;
EVT_IDD_CX_MONITOR_QUERY_TARGET_MODES TargetModes;
EVT_IDD_CX_MONITOR_ASSIGN_SWAPCHAIN Assign;
EVT_IDD_CX_MONITOR_UNASSIGN_SWAPCHAIN Unassign;
uint64_t clockUs(){
 LARGE_INTEGER q{},f{};QueryPerformanceCounter(&q);QueryPerformanceFrequency(&f);
 return static_cast<uint64_t>((q.QuadPart/f.QuadPart)*1000000+(q.QuadPart%f.QuadPart)*1000000/f.QuadPart);
}
struct Worker {
 Device& owner;IDDCX_SWAPCHAIN chain;LUID luid;HANDLE available;
 HANDLE stop=CreateEventW(nullptr,TRUE,FALSE,nullptr);
 std::thread thread;
 Worker(Device& d,IDDCX_SWAPCHAIN c,LUID l,HANDLE a);
 ~Worker(){if(stop)SetEvent(stop);if(thread.joinable())thread.join();if(stop)CloseHandle(stop);}
 void run() noexcept;
};
struct Device {
 WDFDEVICE wdf=nullptr;IDDCX_ADAPTER adapter=nullptr;IDDCX_MONITOR monitor=nullptr;
 std::mutex mutex;std::unique_ptr<Worker> worker;
 std::vector<SdMode> modes;
 std::array<uint8_t,128> edid{};
 SdStatus status{SD_ABI};SdSurfaces surfaces{};UINT surfaceRevision=0;
 bool ready=false;
 HANDLE wake=CreateEventW(nullptr,FALSE,FALSE,nullptr);
 ~Device(){if(wake)CloseHandle(wake);}
 void depart(){
  // IddCx may invoke Unassign synchronously: never hold mutex while notifying it.
  IDDCX_MONITOR old=nullptr;
  {std::lock_guard<std::mutex> guard(mutex);old=monitor;monitor=nullptr;}
  if(old)IddCxMonitorDeparture(old);
  worker.reset();
  std::lock_guard<std::mutex> guard(mutex);
  ++status.generation;status.width=status.height=0;for(auto& s:status.slots)s={};surfaces={};
 }
 void makeEdid(const SdStart& start){
  edid={0x00,0xff,0xff,0xff,0xff,0xff,0xff,0x00,0x4c,0x83,0x01,0x00};
  memcpy(&edid[12],&start.identity,4);edid[16]=1;edid[17]=36;edid[18]=1;edid[19]=4;
  edid[20]=0x80;edid[21]=28;edid[22]=21;edid[23]=120;edid[24]=0x06;
  for(int i=38;i<54;i+=2){edid[i]=1;edid[i+1]=1;}
  for(size_t i=0;i<modes.size();i++){
   const auto m=modes[i];auto* d=&edid[54+i*18];
   const UINT hb=160,vb=30;const UINT clock=(m.width+hb)*(m.height+vb)*m.fps/10000;
   d[0]=static_cast<uint8_t>(clock);d[1]=static_cast<uint8_t>(clock>>8);
   d[2]=static_cast<uint8_t>(m.width);d[3]=hb;d[4]=static_cast<uint8_t>((m.width>>8)<<4);
   d[5]=static_cast<uint8_t>(m.height);d[6]=vb;d[7]=static_cast<uint8_t>((m.height>>8)<<4);
   d[8]=48;d[9]=32;d[10]=0x35;d[17]=0x1e;
  }
  uint8_t sum=0;for(size_t i=0;i<127;i++)sum+=edid[i];edid[127]=static_cast<uint8_t>(0-sum);
 }
 NTSTATUS arrive(const SdStart& start){
  if(!ready||!adapter)return NotReady;
  if(start.abi!=SD_ABI||start.count<1||start.count>SD_MODES)return Invalid;
  for(UINT i=0;i<start.count;i++){const auto& m=start.modes[i];if(m.width<320||m.height<320||m.width>4094||m.height>4094||(m.width&1)||(m.height&1)||m.fps<30||m.fps>60)return Invalid;}
  depart();status.fps=start.modes[0].fps;modes.assign(start.modes,start.modes+start.count);makeEdid(start);
  IDDCX_MONITOR_INFO info{};info.Size=sizeof(info);info.MonitorType=DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INDIRECT_WIRED;
  info.ConnectorIndex=0;info.MonitorContainerId=start.identity;
  info.MonitorDescription.Size=sizeof(IDDCX_MONITOR_DESCRIPTION);info.MonitorDescription.Type=IDDCX_MONITOR_DESCRIPTION_TYPE_EDID;
  info.MonitorDescription.DataSize=static_cast<UINT>(edid.size());info.MonitorDescription.pData=edid.data();
  WDF_OBJECT_ATTRIBUTES attrs;WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attrs,Context);
  IDARG_IN_MONITORCREATE in{};in.ObjectAttributes=&attrs;in.pMonitorInfo=&info;IDARG_OUT_MONITORCREATE out{};
  NTSTATUS result=IddCxMonitorCreate(adapter,&in,&out);if(result<0)return result;
  WdfObjectGet_Context(out.MonitorObject)->device=this;
  {std::lock_guard<std::mutex> guard(mutex);monitor=out.MonitorObject;}
  IDARG_OUT_MONITORARRIVAL arrival{};result=IddCxMonitorArrival(out.MonitorObject,&arrival);
  if(result<0){std::lock_guard<std::mutex> guard(mutex);monitor=nullptr;WdfObjectDelete(out.MonitorObject);}
  return result;
 }
};
Worker::Worker(Device& d,IDDCX_SWAPCHAIN c,LUID l,HANDLE a):owner(d),chain(c),luid(l),available(a){
 if(!stop)throw std::bad_alloc();thread=std::thread([this]{run();});
}
void Worker::run() noexcept {
 auto body=[&](){
  ComPtr<IDXGIFactory4> factory;if(FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))))return;
  ComPtr<IDXGIAdapter> adapter;if(FAILED(factory->EnumAdapterByLuid(luid,IID_PPV_ARGS(&adapter))))return;
  ComPtr<ID3D11Device> device;ComPtr<ID3D11DeviceContext> context;
  if(FAILED(D3D11CreateDevice(adapter.Get(),D3D_DRIVER_TYPE_UNKNOWN,nullptr,D3D11_CREATE_DEVICE_BGRA_SUPPORT,nullptr,0,D3D11_SDK_VERSION,&device,nullptr,&context)))return;
  ComPtr<ID3D11Device1> device1;if(FAILED(device.As(&device1)))return;
  ComPtr<IDXGIDevice> dxgi;if(FAILED(device.As(&dxgi)))return;
  IDARG_IN_SWAPCHAINSETDEVICE set{};set.pDevice=dxgi.Get();if(FAILED(IddCxSwapChainSetDevice(chain,&set)))return;
  std::array<ComPtr<ID3D11Texture2D>,3> shared;
  std::array<ComPtr<IDXGIKeyedMutex>,3> mutexes;
  UINT localRevision=0;uint64_t frame=0;
  ComPtr<ID3D11Texture2D> latest;
  while(WaitForSingleObject(stop,0)==WAIT_TIMEOUT){
   IDARG_OUT_RELEASEANDACQUIREBUFFER out{};
   HRESULT hr=IddCxSwapChainReleaseAndAcquireBuffer(chain,&out);
   bool fresh=SUCCEEDED(hr);
   if(hr==E_PENDING){
    DWORD timeout=INFINITE;
    {std::lock_guard<std::mutex> guard(owner.mutex);if(owner.surfaces.abi==SD_ABI)timeout=1000/std::max<UINT>(owner.status.fps,30);}
    HANDLE events[]{stop,available,owner.wake};DWORD wait=WaitForMultipleObjects(3,events,FALSE,timeout);
    if(wait==WAIT_OBJECT_0||wait==WAIT_FAILED)break;
    if(wait==WAIT_OBJECT_0+1)continue;
    if(!latest)continue;
   }else if(FAILED(hr))break;
   ComPtr<IDXGIResource> surface;
   if(fresh){surface.Attach(out.MetaData.pSurface);if(FAILED(surface.As(&latest)))break;}
   ComPtr<ID3D11Texture2D> texture=latest;
   D3D11_TEXTURE2D_DESC desc{};texture->GetDesc(&desc);
   const uint64_t capture=clockUs();++frame;
   {
    std::lock_guard<std::mutex> guard(owner.mutex);
    if(owner.status.width!=desc.Width||owner.status.height!=desc.Height||
       owner.status.luid_low!=luid.LowPart||owner.status.luid_high!=luid.HighPart){
     owner.status.width=desc.Width;owner.status.height=desc.Height;owner.status.luid_low=luid.LowPart;owner.status.luid_high=luid.HighPart;
     ++owner.status.generation;owner.surfaces={};for(auto& s:owner.status.slots)s={};
    }
    if(localRevision!=owner.surfaceRevision&&owner.surfaces.generation==owner.status.generation){
     for(UINT i=0;i<3;i++){
      shared[i].Reset();mutexes[i].Reset();
      if(SUCCEEDED(device1->OpenSharedResourceByName(owner.surfaces.names[i],DXGI_SHARED_RESOURCE_READ|DXGI_SHARED_RESOURCE_WRITE,IID_PPV_ARGS(&shared[i])))){
       D3D11_TEXTURE2D_DESC sd{};shared[i]->GetDesc(&sd);
       if(sd.Width!=desc.Width||sd.Height!=desc.Height||sd.Format!=desc.Format){shared[i].Reset();continue;}
       shared[i].As(&mutexes[i]);
      }
     }
     localRevision=owner.surfaceRevision;
    }
    for(UINT i=0;i<3;i++){
     if(!mutexes[i])continue;
     HRESULT acquired=mutexes[i]->AcquireSync(0,0);
     if(acquired!=S_OK)continue;
     context->CopyResource(shared[i].Get(),texture.Get());context->Flush();
     if(SUCCEEDED(mutexes[i]->ReleaseSync(1)))owner.status.slots[i]={frame,capture};
     break;
    }
   }
   texture.Reset();surface.Reset();
   if(fresh&&FAILED(IddCxSwapChainFinishedProcessingFrame(chain)))break;
  }
 };
 try{body();}catch(...){OutputDebugStringW(L"SidecarDOS: swapchain worker failed\n");}
 WdfObjectDelete(chain);chain=nullptr;
}
DISPLAYCONFIG_VIDEO_SIGNAL_INFO signal(const SdMode& m,bool monitor){
 DISPLAYCONFIG_VIDEO_SIGNAL_INFO s{};s.activeSize={m.width,m.height};
 s.totalSize={m.width+160,m.height+30};s.pixelRate=static_cast<UINT64>(s.totalSize.cx)*s.totalSize.cy*m.fps;
 s.hSyncFreq={static_cast<UINT32>(s.pixelRate),s.totalSize.cx};
 s.vSyncFreq={m.fps,1};s.scanLineOrdering=DISPLAYCONFIG_SCANLINE_ORDERING_PROGRESSIVE;
 if(!monitor)s.AdditionalSignalInfo.vSyncFreqDivider=1;
 return s;
}
IDDCX_MONITOR_MODE monitorMode(const SdMode& m){IDDCX_MONITOR_MODE out{};out.Size=sizeof(out);out.Origin=IDDCX_MONITOR_MODE_ORIGIN_MONITORDESCRIPTOR;out.MonitorVideoSignalInfo=signal(m,true);return out;}

NTSTATUS AdapterReady(IDDCX_ADAPTER a,const IDARG_IN_ADAPTER_INIT_FINISHED* in){
 auto* d=WdfObjectGet_Context(a)->device;d->ready=in->AdapterInitStatus>=0;return Success;
}
NTSTATUS Commit(IDDCX_ADAPTER,const IDARG_IN_COMMITMODES*){return Success;}
NTSTATUS Parse(const IDARG_IN_PARSEMONITORDESCRIPTION* in,IDARG_OUT_PARSEMONITORDESCRIPTION* out){
 if(in->MonitorDescription.DataSize!=128)return Invalid;
 const auto* edid=in->MonitorDescription.pData;std::vector<SdMode> modes;
 for(UINT i=0;i<4;i++){
  const auto* d=edid+54+18*i;UINT clock=d[0]|(d[1]<<8);if(!clock)continue;
  UINT w=d[2]|((d[4]>>4)<<8),h=d[5]|((d[7]>>4)<<8);
  UINT hb=d[3]|((d[4]&15)<<8),vb=d[6]|((d[7]&15)<<8);
  if(!w||!h||!hb||!vb)return Invalid;
  UINT fps=static_cast<UINT>((clock*10000.0/((w+hb)*(h+vb)))+0.5);
  modes.push_back({w,h,fps});
 }
 if(modes.empty())return Invalid;
 out->MonitorModeBufferOutputCount=static_cast<UINT>(modes.size());out->PreferredMonitorModeIdx=0;
 if(!in->MonitorModeBufferInputCount)return Success;
 if(in->MonitorModeBufferInputCount<modes.size())return static_cast<NTSTATUS>(0xc0000023);
 for(size_t i=0;i<modes.size();i++)in->pMonitorModes[i]=monitorMode(modes[i]);return Success;
}
NTSTATUS DefaultModes(IDDCX_MONITOR,const IDARG_IN_GETDEFAULTDESCRIPTIONMODES*,IDARG_OUT_GETDEFAULTDESCRIPTIONMODES* out){
 out->DefaultMonitorModeBufferOutputCount=0;return Success;
}
NTSTATUS TargetModes(IDDCX_MONITOR m,const IDARG_IN_QUERYTARGETMODES* in,IDARG_OUT_QUERYTARGETMODES* out){
 auto* d=WdfObjectGet_Context(m)->device;out->TargetModeBufferOutputCount=static_cast<UINT>(d->modes.size());
 if(!in->TargetModeBufferInputCount)return Success;
 if(in->TargetModeBufferInputCount<d->modes.size())return static_cast<NTSTATUS>(0xc0000023);
 for(size_t i=0;i<d->modes.size();i++){auto& o=in->pTargetModes[i];o={};o.Size=sizeof(o);o.TargetVideoSignalInfo.targetVideoSignalInfo=signal(d->modes[i],false);}
 return Success;
}
NTSTATUS Assign(IDDCX_MONITOR m,const IDARG_IN_SETSWAPCHAIN* in){
 auto* d=WdfObjectGet_Context(m)->device;d->worker.reset();
 {std::lock_guard<std::mutex> guard(d->mutex);++d->status.generation;d->status.width=d->status.height=0;d->surfaces={};for(auto& s:d->status.slots)s={};}
 try{d->worker=std::make_unique<Worker>(*d,in->hSwapChain,in->RenderAdapterLuid,in->hNextSurfaceAvailable);}catch(...){WdfObjectDelete(in->hSwapChain);return static_cast<NTSTATUS>(0xc000009a);}
 return Success;
}
NTSTATUS Unassign(IDDCX_MONITOR m){auto* d=WdfObjectGet_Context(m)->device;d->worker.reset();return Success;}
void IoControl(WDFDEVICE device,WDFREQUEST request,size_t outSize,size_t inSize,ULONG code){
 auto* d=WdfObjectGet_Context(device)->device;NTSTATUS result=Invalid;ULONG_PTR bytes=0;
 try {
  if(code==SD_START&&inSize==sizeof(SdStart)){
   SdStart* in=nullptr;result=WdfRequestRetrieveInputBuffer(request,sizeof(SdStart),reinterpret_cast<void**>(&in),nullptr);
   if(result>=0)result=d->arrive(*in);
  }else if(code==SD_STOP){d->depart();result=Success;}
  else if(code==SD_STATUS&&outSize>=sizeof(SdStatus)){
   SdStatus* out=nullptr;result=WdfRequestRetrieveOutputBuffer(request,sizeof(SdStatus),reinterpret_cast<void**>(&out),nullptr);
   if(result>=0){std::lock_guard<std::mutex> guard(d->mutex);*out=d->status;bytes=sizeof(SdStatus);}
  }else if(code==SD_SURFACES&&inSize==sizeof(SdSurfaces)){
   SdSurfaces* in=nullptr;result=WdfRequestRetrieveInputBuffer(request,sizeof(SdSurfaces),reinterpret_cast<void**>(&in),nullptr);
   if(result>=0){
    std::lock_guard<std::mutex> guard(d->mutex);result=Invalid;
    bool valid=in->abi==SD_ABI&&in->generation==d->status.generation;
    for(const auto& n:in->names)valid=valid&&n[SD_NAME-1]==0&&wcsncmp(n,L"Global\\SidecarDOS.",18)==0;
    if(valid){d->surfaces=*in;++d->surfaceRevision;if(d->wake)SetEvent(d->wake);result=Success;}
   }
  }
 }catch(...){result=static_cast<NTSTATUS>(0xc000009a);}
 WdfRequestCompleteWithInformation(request,result,bytes);
}
void FileCleanup(WDFFILEOBJECT f){WdfObjectGet_Context(WdfFileObjectGetDevice(f))->device->depart();}
void Cleanup(WDFOBJECT obj){auto* c=WdfObjectGet_Context(obj);if(c->device){c->device->depart();delete c->device;c->device=nullptr;}}
NTSTATUS EnterD0(WDFDEVICE device,WDF_POWER_DEVICE_STATE){
 auto* d=WdfObjectGet_Context(device)->device;
 IDDCX_ADAPTER_CAPS caps{};caps.Size=sizeof(caps);caps.MaxMonitorsSupported=1;
 caps.EndPointDiagnostics.Size=sizeof(caps.EndPointDiagnostics);caps.EndPointDiagnostics.GammaSupport=IDDCX_FEATURE_IMPLEMENTATION_NONE;
 caps.EndPointDiagnostics.TransmissionType=IDDCX_TRANSMISSION_TYPE_WIRED_OTHER;
 caps.EndPointDiagnostics.pEndPointFriendlyName=L"SidecarDOS Virtual Display";
 caps.EndPointDiagnostics.pEndPointManufacturerName=L"SidecarDOS";caps.EndPointDiagnostics.pEndPointModelName=L"SidecarDOS v1";
 WDF_OBJECT_ATTRIBUTES attrs;WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attrs,Context);
 IDDCX_ENDPOINT_VERSION version{};version.Size=sizeof(version);version.MajorVer=1;
 caps.EndPointDiagnostics.pFirmwareVersion=&version;caps.EndPointDiagnostics.pHardwareVersion=&version;
 IDARG_IN_ADAPTER_INIT in{};in.WdfDevice=device;in.pCaps=&caps;in.ObjectAttributes=&attrs;
 IDARG_OUT_ADAPTER_INIT out{};NTSTATUS result=IddCxAdapterInitAsync(&in,&out);
 if(result>=0){d->adapter=out.AdapterObject;WdfObjectGet_Context(out.AdapterObject)->device=d;}return result;
}
NTSTATUS ExitD0(WDFDEVICE device,WDF_POWER_DEVICE_STATE){auto* d=WdfObjectGet_Context(device)->device;d->ready=false;d->depart();return Success;}
NTSTATUS AddDevice(WDFDRIVER,PWDFDEVICE_INIT init){
 WdfDeviceInitSetExclusive(init,TRUE);
 WDF_FILEOBJECT_CONFIG file;WDF_FILEOBJECT_CONFIG_INIT(&file,WDF_NO_EVENT_CALLBACK,WDF_NO_EVENT_CALLBACK,FileCleanup);
 WdfDeviceInitSetFileObjectConfig(init,&file,WDF_NO_OBJECT_ATTRIBUTES);
 WDF_PNPPOWER_EVENT_CALLBACKS pnp;WDF_PNPPOWER_EVENT_CALLBACKS_INIT(&pnp);pnp.EvtDeviceD0Entry=EnterD0;pnp.EvtDeviceD0Exit=ExitD0;
 WdfDeviceInitSetPnpPowerEventCallbacks(init,&pnp);
 IDD_CX_CLIENT_CONFIG config;IDD_CX_CLIENT_CONFIG_INIT(&config);
 config.EvtIddCxDeviceIoControl=IoControl;
 config.EvtIddCxAdapterInitFinished=AdapterReady;config.EvtIddCxAdapterCommitModes=Commit;
 config.EvtIddCxParseMonitorDescription=Parse;config.EvtIddCxMonitorGetDefaultDescriptionModes=DefaultModes;
 config.EvtIddCxMonitorQueryTargetModes=TargetModes;config.EvtIddCxMonitorAssignSwapChain=Assign;config.EvtIddCxMonitorUnassignSwapChain=Unassign;
 NTSTATUS result=IddCxDeviceInitConfig(init,&config);if(result<0)return result;
 WDF_OBJECT_ATTRIBUTES attrs;WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&attrs,Context);attrs.EvtCleanupCallback=Cleanup;
 attrs.SynchronizationScope=WdfSynchronizationScopeDevice;
 WDFDEVICE device;result=WdfDeviceCreate(&init,&attrs,&device);if(result<0)return result;
 auto* c=WdfObjectGet_Context(device);c->device=new(std::nothrow)Device;if(!c->device)return static_cast<NTSTATUS>(0xc000009a);
 c->device->wdf=device;
 result=IddCxDeviceInitialize(device);if(result<0)return result;
 result=WdfDeviceCreateDeviceInterface(device,&SIDECARDOS_INTERFACE,nullptr);if(result<0)return result;
 return Success;
}
}
extern "C" DRIVER_INITIALIZE DriverEntry;
extern "C" NTSTATUS DriverEntry(PDRIVER_OBJECT driver,PUNICODE_STRING registry){
 WDF_DRIVER_CONFIG config;WDF_DRIVER_CONFIG_INIT(&config,AddDevice);
 return WdfDriverCreate(driver,registry,WDF_NO_OBJECT_ATTRIBUTES,&config,WDF_NO_HANDLE);
}
