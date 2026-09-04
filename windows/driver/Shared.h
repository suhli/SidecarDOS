#pragma once
#include <Windows.h>
#include <winioctl.h>
#include <cstdint>

inline constexpr GUID SIDECARDOS_INTERFACE = {
    0x777d8d93, 0x591a, 0x44f4, {0x8b, 0x3c, 0x75, 0x6e, 0x20, 0xdd, 0x9c, 0x58}};
constexpr uint32_t SD_ABI = 1;
constexpr uint32_t SD_SLOTS = 3;
constexpr uint32_t SD_MODES = 4;
constexpr uint32_t SD_NAME = 128;
constexpr DWORD SD_START = CTL_CODE(FILE_DEVICE_UNKNOWN, 0x800, METHOD_BUFFERED, FILE_WRITE_DATA);
constexpr DWORD SD_STOP = CTL_CODE(FILE_DEVICE_UNKNOWN, 0x801, METHOD_BUFFERED, FILE_WRITE_DATA);
constexpr DWORD SD_STATUS = CTL_CODE(FILE_DEVICE_UNKNOWN, 0x802, METHOD_BUFFERED, FILE_READ_DATA);
constexpr DWORD SD_SURFACES =
    CTL_CODE(FILE_DEVICE_UNKNOWN, 0x803, METHOD_BUFFERED, FILE_WRITE_DATA);
#pragma pack(push, 8)
struct SdMode
{
    uint32_t width, height, fps;
};
struct SdStart
{
    uint32_t abi, count;
    GUID identity;
    SdMode modes[SD_MODES];
};
struct SdSlot
{
    uint64_t frame, capture_us;
};
struct SdStatus
{
    uint32_t abi, generation, width, height, fps, luid_low;
    int32_t luid_high;
    uint32_t reserved;
    SdSlot slots[SD_SLOTS];
};
struct SdSurfaces
{
    uint32_t abi, generation;
    wchar_t names[SD_SLOTS][SD_NAME];
};
#pragma pack(pop)
static_assert(sizeof(SdStatus) == 80);
static_assert(sizeof(SdStart) == 72);
