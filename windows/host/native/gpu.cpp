#include <Windows.h>
#include <d3d11_1.h>
#include <dxgi1_4.h>
#include <d3d10.h>
#include <mfapi.h>
#include <mfidl.h>
#include <mferror.h>
#include <codecapi.h>
#include <wmcodecdsp.h>
#include <sddl.h>
#include <wrl/client.h>
#include <array>
#include <map>
#include <memory>
#include <string>
#include <vector>
#include <algorithm>
#include <stdexcept>
using Microsoft::WRL::ComPtr;
namespace {
thread_local char errorText[768]{};
void checked(HRESULT hr,const char* expression,int line) { if(FAILED(hr)){sprintf_s(errorText,"%s (line %d): HRESULT 0x%08lx",expression,line,static_cast<unsigned long>(hr));throw hr;} }
#define check(expression) checked((expression),#expression,__LINE__)
struct Handle {
 HANDLE value=nullptr;
 ~Handle(){if(value)CloseHandle(value);}
 Handle()=default;Handle(const Handle&)=delete;Handle& operator=(const Handle&)=delete;
};
struct Security {
 PSECURITY_DESCRIPTOR descriptor=nullptr;
 Security(){check(ConvertStringSecurityDescriptorToSecurityDescriptorW(
  L"D:P(A;;GA;;;SY)(A;;GA;;;LS)(A;;GA;;;OW)",SDDL_REVISION_1,&descriptor,nullptr)
  ? S_OK : HRESULT_FROM_WIN32(GetLastError()));}
 ~Security(){if(descriptor)LocalFree(descriptor);}
};
struct MutexLease {
 ComPtr<IDXGIKeyedMutex> mutex; bool owned=false;
 ~MutexLease(){if(owned)mutex->ReleaseSync(0);}
};
struct Encoder {
 bool com=false,mf=false;
 ComPtr<ID3D11Device> device;
 ComPtr<ID3D11DeviceContext> context;
 ComPtr<ID3D11VideoDevice> video;
 ComPtr<ID3D11VideoContext> videoContext;
 ComPtr<ID3D11VideoProcessorEnumerator> enumerator;
 ComPtr<ID3D11VideoProcessor> processor;
 ComPtr<IMFDXGIDeviceManager> manager;
 ComPtr<IMFTransform> transform;
 ComPtr<IMFMediaEventGenerator> events;
 ComPtr<ICodecAPI> codec;
 std::array<ComPtr<ID3D11Texture2D>,3> shared;
 std::array<ComPtr<IDXGIKeyedMutex>,3> mutexes;
 std::array<Handle,3> handles;
 std::map<LONGLONG,std::pair<uint64_t,uint64_t>> pending;
 UINT width=0,height=0,fps=60,credits=0,outputs=0;
 DWORD inputId=0,outputId=0;
 ~Encoder(){
  if(transform){transform->ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH,0);ComPtr<IMFShutdown> shutdown;if(SUCCEEDED(transform.As(&shutdown)))shutdown->Shutdown();}
  events.Reset();codec.Reset();transform.Reset();pending.clear();manager.Reset();
  processor.Reset();enumerator.Reset();videoContext.Reset();video.Reset();
  for(auto& m:mutexes)m.Reset();for(auto& t:shared)t.Reset();
  context.Reset();device.Reset();
  if(mf)MFShutdown();if(com)CoUninitialize();
 }
 void set(const GUID& key,ULONG value,bool required=true){
  VARIANT v;VariantInit(&v);v.vt=VT_UI4;v.ulVal=value;HRESULT hr=codec->SetValue(&key,&v);
  if(required)check(hr);
 }
 void flag(const GUID& key,bool enabled,bool required=true){
  VARIANT v;VariantInit(&v);v.vt=VT_BOOL;v.boolVal=enabled?VARIANT_TRUE:VARIANT_FALSE;
  HRESULT hr=codec->SetValue(&key,&v);if(required)check(hr);
 }
 void media(ComPtr<IMFMediaType>& type,const GUID& subtype){
  check(MFCreateMediaType(&type));check(type->SetGUID(MF_MT_MAJOR_TYPE,MFMediaType_Video));
  check(type->SetGUID(MF_MT_SUBTYPE,subtype));
  check(MFSetAttributeSize(type.Get(),MF_MT_FRAME_SIZE,width,height));
  check(MFSetAttributeRatio(type.Get(),MF_MT_FRAME_RATE,fps,1));
  check(MFSetAttributeRatio(type.Get(),MF_MT_PIXEL_ASPECT_RATIO,1,1));
  check(type->SetUINT32(MF_MT_INTERLACE_MODE,MFVideoInterlace_Progressive));
  check(type->SetUINT32(MF_MT_VIDEO_PRIMARIES,MFVideoPrimaries_BT709));
  check(type->SetUINT32(MF_MT_TRANSFER_FUNCTION,MFVideoTransFunc_709));
  check(type->SetUINT32(MF_MT_YUV_MATRIX,MFVideoTransferMatrix_BT709));
  check(type->SetUINT32(MF_MT_VIDEO_NOMINAL_RANGE,MFNominalRange_16_235));
 }
 void activate(IMFActivate* activation,UINT bitrate){
  check(activation->ActivateObject(IID_PPV_ARGS(&transform)));
  ComPtr<IMFAttributes> a;check(transform->GetAttributes(&a));
  UINT32 aware=0;check(a->GetUINT32(MF_SA_D3D11_AWARE,&aware));if(!aware)throw MF_E_UNSUPPORTED_D3D_TYPE;
  check(a->SetUINT32(MF_TRANSFORM_ASYNC_UNLOCK,TRUE));
  check(a->SetUINT32(MF_LOW_LATENCY,TRUE));
  check(transform.As(&codec));check(transform.As(&events));
  check(transform->ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER,reinterpret_cast<ULONG_PTR>(manager.Get())));
  DWORD inCount=0,outCount=0;check(transform->GetStreamCount(&inCount,&outCount));if(inCount!=1||outCount!=1)throw E_NOTIMPL;
  HRESULT ids=transform->GetStreamIDs(1,&inputId,1,&outputId);if(ids==E_NOTIMPL){inputId=outputId=0;}else check(ids);
  set(CODECAPI_AVEncCommonRateControlMode,eAVEncCommonRateControlMode_CBR);
  set(CODECAPI_AVEncCommonMeanBitRate,bitrate);
  flag(CODECAPI_AVLowLatencyMode,true);
  set(CODECAPI_AVEncMPVDefaultBPictureCount,0);
  set(CODECAPI_AVEncMPVGOPSize,fps*2,false);
  set(CODECAPI_AVEncCommonRealTime,1,false);
  ComPtr<IMFMediaType> out;media(out,MFVideoFormat_H264);
  check(out->SetUINT32(MF_MT_AVG_BITRATE,bitrate));
  check(out->SetUINT32(MF_MT_MPEG2_PROFILE,eAVEncH264VProfile_Base));
  check(transform->SetOutputType(outputId,out.Get(),0));
  ComPtr<IMFMediaType> in;media(in,MFVideoFormat_NV12);check(transform->SetInputType(inputId,in.Get(),0));
  check(transform->ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,0));
  check(transform->ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM,0));
 }
 void init(UINT low,LONG high,UINT w,UINT h,UINT rate,UINT bitrate,const wchar_t* prefix){
  check(CoInitializeEx(nullptr,COINIT_MULTITHREADED));com=true;check(MFStartup(MF_VERSION,MFSTARTUP_LITE));mf=true;
  width=w;height=h;fps=rate;
  ComPtr<IDXGIFactory4> factory;check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
  LUID luid{low,high};ComPtr<IDXGIAdapter> adapter;check(factory->EnumAdapterByLuid(luid,IID_PPV_ARGS(&adapter)));
  check(D3D11CreateDevice(adapter.Get(),D3D_DRIVER_TYPE_UNKNOWN,nullptr,
   D3D11_CREATE_DEVICE_BGRA_SUPPORT|D3D11_CREATE_DEVICE_VIDEO_SUPPORT,nullptr,0,D3D11_SDK_VERSION,&device,nullptr,&context));
  ComPtr<ID3D10Multithread> mt;check(context.As(&mt));mt->SetMultithreadProtected(TRUE);
  check(device.As(&video));check(context.As(&videoContext));
  UINT token=0;check(MFCreateDXGIDeviceManager(&token,&manager));check(manager->ResetDevice(device.Get(),token));
  D3D11_VIDEO_PROCESSOR_CONTENT_DESC desc{};desc.InputFrameFormat=D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE;
  desc.InputFrameRate={fps,1};desc.OutputFrameRate={fps,1};desc.InputWidth=desc.OutputWidth=w;desc.InputHeight=desc.OutputHeight=h;
  desc.Usage=D3D11_VIDEO_USAGE_PLAYBACK_NORMAL;
  check(video->CreateVideoProcessorEnumerator(&desc,&enumerator));check(video->CreateVideoProcessor(enumerator.Get(),0,&processor));
  D3D11_VIDEO_PROCESSOR_COLOR_SPACE rgb{};rgb.RGB_Range=0;rgb.YCbCr_Matrix=1;
  D3D11_VIDEO_PROCESSOR_COLOR_SPACE yuv{};yuv.YCbCr_Matrix=1;yuv.Nominal_Range=1;
  videoContext->VideoProcessorSetStreamColorSpace(processor.Get(),0,&rgb);
  videoContext->VideoProcessorSetOutputColorSpace(processor.Get(),&yuv);
  videoContext->VideoProcessorSetStreamAutoProcessingMode(processor.Get(),0,FALSE);
  Security security;SECURITY_ATTRIBUTES sa{sizeof(sa),security.descriptor,FALSE};
  D3D11_TEXTURE2D_DESC td{};td.Width=w;td.Height=h;td.MipLevels=1;td.ArraySize=1;td.Format=DXGI_FORMAT_B8G8R8A8_UNORM;
  td.SampleDesc.Count=1;td.Usage=D3D11_USAGE_DEFAULT;td.BindFlags=D3D11_BIND_RENDER_TARGET|D3D11_BIND_SHADER_RESOURCE;
  td.MiscFlags=D3D11_RESOURCE_MISC_SHARED_NTHANDLE|D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX;
  for(UINT i=0;i<3;i++){
   check(device->CreateTexture2D(&td,nullptr,&shared[i]));check(shared[i].As(&mutexes[i]));
   ComPtr<IDXGIResource1> resource;check(shared[i].As(&resource));
   auto name=std::wstring(prefix)+L"."+std::to_wstring(i);
   check(resource->CreateSharedHandle(&sa,DXGI_SHARED_RESOURCE_READ|DXGI_SHARED_RESOURCE_WRITE,name.c_str(),&handles[i].value));
  }
  ComPtr<IMFAttributes> attrs;check(MFCreateAttributes(&attrs,1));
  UINT64 adapterId=static_cast<UINT64>(low)|(static_cast<UINT64>(static_cast<UINT32>(high))<<32);
  check(attrs->SetUINT64(MFT_ENUM_ADAPTER_LUID,adapterId));
  MFT_REGISTER_TYPE_INFO input{MFMediaType_Video,MFVideoFormat_NV12},output{MFMediaType_Video,MFVideoFormat_H264};
  IMFActivate** list=nullptr;UINT count=0;
  check(MFTEnum2(MFT_CATEGORY_VIDEO_ENCODER,MFT_ENUM_FLAG_HARDWARE|MFT_ENUM_FLAG_SORTANDFILTER,&input,&output,attrs.Get(),&list,&count));
  HRESULT result=MF_E_TOPO_CODEC_NOT_FOUND;
  for(UINT i=0;i<count;i++){
   if(FAILED(result)){
    try{activate(list[i],bitrate);result=S_OK;}
    catch(HRESULT hr){result=hr;events.Reset();codec.Reset();transform.Reset();list[i]->ShutdownObject();}
   }
   list[i]->Release();
  }
  CoTaskMemFree(list);if(FAILED(result))throw result;
 }
 void pump(){
  for(UINT i=0;i<32;i++){
   ComPtr<IMFMediaEvent> event;HRESULT hr=events->GetEvent(MF_EVENT_FLAG_NO_WAIT,&event);
   if(hr==MF_E_NO_EVENTS_AVAILABLE)break;check(hr);
   HRESULT status=S_OK;check(event->GetStatus(&status));check(status);
   MediaEventType type;check(event->GetType(&type));
   if(type==METransformNeedInput)credits=std::min<UINT>(credits+1,3);
   else if(type==METransformHaveOutput)outputs=std::min<UINT>(outputs+1,4);
   else if(type==MEError)throw E_FAIL;
  }
 }
 HRESULT input(UINT slot,uint64_t frame,uint64_t capture){
  if(slot>=3)return E_INVALIDARG;
  MutexLease lease;lease.mutex=mutexes[slot];HRESULT acquired=lease.mutex->AcquireSync(1,0);
  if(acquired==WAIT_TIMEOUT)return S_FALSE;check(acquired);lease.owned=true;
  pump();if(frame==0||!credits||pending.size()>=3)return S_FALSE;
  D3D11_TEXTURE2D_DESC td{};td.Width=width;td.Height=height;td.MipLevels=1;td.ArraySize=1;td.Format=DXGI_FORMAT_NV12;
  td.SampleDesc.Count=1;td.BindFlags=D3D11_BIND_RENDER_TARGET;td.Usage=D3D11_USAGE_DEFAULT;
  ComPtr<ID3D11Texture2D> nv12;check(device->CreateTexture2D(&td,nullptr,&nv12));
  D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC iv{};iv.ViewDimension=D3D11_VPIV_DIMENSION_TEXTURE2D;
  ComPtr<ID3D11VideoProcessorInputView> inputView;check(video->CreateVideoProcessorInputView(shared[slot].Get(),enumerator.Get(),&iv,&inputView));
  D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC ov{};ov.ViewDimension=D3D11_VPOV_DIMENSION_TEXTURE2D;
  ComPtr<ID3D11VideoProcessorOutputView> outputView;check(video->CreateVideoProcessorOutputView(nv12.Get(),enumerator.Get(),&ov,&outputView));
  D3D11_VIDEO_PROCESSOR_STREAM stream{};stream.Enable=TRUE;stream.pInputSurface=inputView.Get();
  check(videoContext->VideoProcessorBlt(processor.Get(),outputView.Get(),0,1,&stream));
  context->Flush(); // ReleaseSync serializes shared surface access across the two processes.
  check(lease.mutex->ReleaseSync(0));lease.owned=false;
  ComPtr<IMFMediaBuffer> buffer;check(MFCreateDXGISurfaceBuffer(__uuidof(ID3D11Texture2D),nv12.Get(),0,FALSE,&buffer));
  ComPtr<IMFSample> sample;check(MFCreateSample(&sample));check(sample->AddBuffer(buffer.Get()));
  LONGLONG time=static_cast<LONGLONG>(capture*10);check(sample->SetSampleTime(time));check(sample->SetSampleDuration(10000000/fps));
  check(transform->ProcessInput(inputId,sample.Get(),0));--credits;pending.emplace(time,std::make_pair(frame,capture));return S_OK;
 }
};
struct NativeOutput {uint64_t frame,capture_us;uint32_t keyframe,size;};
}
extern "C" int32_t sd_gpu_create(uint32_t low,int32_t high,uint32_t w,uint32_t h,uint32_t fps,uint32_t bitrate,const wchar_t* prefix,void** out){
 if(!out||!prefix||w<320||h<320||w>4096||h>4096||(w&1)||(h&1)||fps<30||fps>60)return E_INVALIDARG;
 *out=nullptr;try{auto e=std::make_unique<Encoder>();e->init(low,high,w,h,fps,bitrate,prefix);*out=e.release();return S_OK;}catch(HRESULT hr){return hr;}catch(...){return E_OUTOFMEMORY;}
}
extern "C" void sd_gpu_destroy(void* p){delete static_cast<Encoder*>(p);}
extern "C" int32_t sd_gpu_input(void* p,uint32_t slot,uint64_t frame,uint64_t capture){
 try{return static_cast<Encoder*>(p)->input(slot,frame,capture);}catch(HRESULT hr){return hr;}catch(...){return E_FAIL;}
}
extern "C" int32_t sd_gpu_bitrate(void* p,uint32_t bitrate){
 try{static_cast<Encoder*>(p)->set(CODECAPI_AVEncCommonMeanBitRate,bitrate);return S_OK;}catch(HRESULT hr){return hr;}catch(...){return E_FAIL;}
}
extern "C" int32_t sd_gpu_keyframe(void* p){
 try{static_cast<Encoder*>(p)->set(CODECAPI_AVEncVideoForceKeyFrame,1);return S_OK;}catch(HRESULT hr){return hr;}catch(...){return E_FAIL;}
}
extern "C" int32_t sd_gpu_poll(void* p,uint8_t* bytes,uint32_t capacity,NativeOutput* out){
 try{
  auto& e=*static_cast<Encoder*>(p);e.pump();if(!e.outputs)return S_FALSE;
  MFT_OUTPUT_STREAM_INFO info{};check(e.transform->GetOutputStreamInfo(e.outputId,&info));
  ComPtr<IMFSample> allocated;ComPtr<IMFMediaBuffer> allocatedBuffer;
  if(!(info.dwFlags&MFT_OUTPUT_STREAM_PROVIDES_SAMPLES)){
   check(MFCreateSample(&allocated));check(MFCreateMemoryBuffer(std::max<DWORD>(info.cbSize,capacity),&allocatedBuffer));
   check(allocated->AddBuffer(allocatedBuffer.Get()));
  }
  MFT_OUTPUT_DATA_BUFFER b{};b.dwStreamID=e.outputId;b.pSample=allocated.Get();DWORD status=0;
  HRESULT result=e.transform->ProcessOutput(0,1,&b,&status);--e.outputs;
  if(b.pEvents)b.pEvents->Release();
  ComPtr<IMFSample> sample;if(b.pSample==allocated.Get())sample=allocated;else sample.Attach(b.pSample);
  if(result==MF_E_TRANSFORM_STREAM_CHANGE){
   ComPtr<IMFMediaType> type;check(e.transform->GetOutputAvailableType(e.outputId,0,&type));check(e.transform->SetOutputType(e.outputId,type.Get(),0));return S_FALSE;
  }
  check(result);if(!sample)return E_UNEXPECTED;
  LONGLONG timestamp=0;check(sample->GetSampleTime(&timestamp));
  auto entry=e.pending.find(timestamp);if(entry==e.pending.end())return E_UNEXPECTED;
  out->frame=entry->second.first;out->capture_us=entry->second.second;e.pending.erase(entry);
  UINT32 clean=0;sample->GetUINT32(MFSampleExtension_CleanPoint,&clean);out->keyframe=clean;
  ComPtr<IMFMediaBuffer> buffer;check(sample->ConvertToContiguousBuffer(&buffer));
  BYTE* data=nullptr;DWORD length=0;check(buffer->Lock(&data,nullptr,&length));
  if(length>capacity){buffer->Unlock();return HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER);}
  memcpy(bytes,data,length);check(buffer->Unlock());out->size=length;return S_OK;
 }catch(HRESULT hr){return hr;}catch(...){return E_FAIL;}
}

extern "C" int32_t sd_gpu_self_test(){
 try{
  ComPtr<IDXGIFactory4> factory;check(CreateDXGIFactory1(IID_PPV_ARGS(&factory)));
  ComPtr<IDXGIAdapter1> adapter;check(factory->EnumAdapters1(0,&adapter));
  DXGI_ADAPTER_DESC1 desc{};check(adapter->GetDesc1(&desc));
  GUID guid;check(CoCreateGuid(&guid));wchar_t name[80]{};
  swprintf_s(name,L"Global\\SidecarDOS.Test.%08x%04x%04x",guid.Data1,guid.Data2,guid.Data3);
  Encoder e;e.init(desc.AdapterLuid.LowPart,desc.AdapterLuid.HighPart,1280,720,60,6000000,name);
  e.set(CODECAPI_AVEncVideoForceKeyFrame,1);
  std::vector<uint8_t> output(2*1024*1024);NativeOutput meta{};
  for(uint64_t i=1;i<=180;i++){
   auto slot=static_cast<UINT>(i%3);
   HRESULT acquired=e.mutexes[slot]->AcquireSync(0,0);
   if(acquired==S_OK){
    ComPtr<ID3D11RenderTargetView> view;check(e.device->CreateRenderTargetView(e.shared[slot].Get(),nullptr,&view));
    const float color[]{static_cast<float>(i%60)/60.0f,0.2f,0.4f,1.0f};
    e.context->ClearRenderTargetView(view.Get(),color);e.context->Flush();check(e.mutexes[slot]->ReleaseSync(1));
    check(e.input(slot,i,i*16667));
   }
   Sleep(10);
   HRESULT hr=sd_gpu_poll(&e,output.data(),static_cast<uint32_t>(output.size()),&meta);check(hr);
   if(hr==S_OK&&meta.size>0){
    for(size_t j=0;j+4<meta.size;j++){
     if(output[j]==0&&output[j+1]==0&&output[j+2]==1&&(output[j+3]&31)==5)return S_OK;
    }
   }
  }
  return HRESULT_FROM_WIN32(ERROR_TIMEOUT);
 }catch(HRESULT hr){return hr;}catch(...){return E_FAIL;}
}

extern "C" const char* sd_gpu_error(){return errorText;}
