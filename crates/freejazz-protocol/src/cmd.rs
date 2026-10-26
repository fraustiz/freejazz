/// Every command of `docs/protocol.md` section 4.
/// The OTA names other than 0x80 and 0x84 are placeholders (the doc does not name them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Cmd {
    CommunicationStart = 0x01,
    CommunicationEnd = 0x02,
    SetFactoryReset = 0x0F,
    GetDeviceInfo = 0x10,
    GetGameMode = 0x11,
    GetKey = 0x12,
    GetLedEffect = 0x13,
    GetCustomLedData = 0x14,
    GetMacro = 0x15,
    GetFnKey = 0x16,
    GetMagneticAxisRt = 0x17,
    GetMagneticAxisDksData = 0x18,
    GetLightBox = 0x1B,
    GetDefaultFnKeyMatrix = 0x1C,
    GetSideLight = 0x1D,
    GetTriggerKey = 0x1E,
    GetDefaultKeyMatrix = 0x1F,
    SetGameMode = 0x21,
    SetKey = 0x22,
    SetLedEffect = 0x23,
    SetCustomLedData = 0x24,
    SetMacro = 0x25,
    SetFnKey = 0x26,
    SetMagneticAxisRt = 0x27,
    SetMagneticAxisDksData = 0x28,
    SetDotMatrix = 0x2A,
    SetLightBox = 0x2B,
    SetSideLight = 0x2D,
    SetTriggerKey = 0x2E,
    SetKeyboardCustomFunctionOn = 0x30,
    SetKeyboardCustomFunctionOff = 0x31,
    GetLedData = 0x32,
    GetAllLightsRgb = 0x33,
    SetTemporaryCommandData = 0x34,
    SetMusicData = 0x35,
    ClearLedData = 0x36,
    ClearLedData24G = 0x37,
    ClearLedData24GB = 0x3B,
    ClearLedData24GC = 0x3C,
    GetDotMatrix = 0x3D,
    SetLedBootAnimation = 0x40,
    SetLedUserAnimation = 0x41,
    SetLedData = 0x42,
    SetFlashDownload = 0x4F,
    SetTft50 = 0x50,
    SetTft51 = 0x51,
    GetMagneticAxisKeyStatus = 0x60,
    SetCalibrationOn = 0x64,
    SetCalibrationOff = 0x65,
    SetSimulationTestOn = 0x66,
    SetSimulationTestOff = 0x67,
    GetMagneticAxisStatus = 0x68,
    SetCalibrationOnV2 = 0x69,
    SetCalibrationOffV2 = 0x6A,
    OtaDeviceEnterBoot = 0x80,
    Ota81 = 0x81,
    Ota82 = 0x82,
    Ota83 = 0x83,
    OtaGetDeviceSn = 0x84,
    Ota85 = 0x85,
    Ota86 = 0x86,
    SetProfile = 0x90,
    GetDeviceNotify = 0xFA,
    GetMagneticAxisCalibrationData = 0xFB,
    Get24gDisconnectNotify = 0xFC,
}

impl Cmd {
    /// Commands that must never be sent: OTA 0x80-0x86, flash download, TFT, LED
    /// animations and raw LED data (Global Constraints).
    pub fn is_denied(self) -> bool {
        Self::is_denied_code(self as u8)
    }

    /// Same deny-list on a raw command byte.
    pub fn is_denied_code(code: u8) -> bool {
        matches!(code, 0x80..=0x86 | 0x4F | 0x50 | 0x51 | 0x41 | 0x42 | 0x40)
    }
}
