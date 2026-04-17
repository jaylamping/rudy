// RS03 communication types (ADR-0002).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CommType {
    GetDeviceId = 0x00,
    OperationCtrl = 0x01,
    MotorFeedback = 0x02,
    Enable = 0x03,
    Stop = 0x04,
    SetZero = 0x06,
    SetCanId = 0x07,
    ReadParam = 0x11,
    WriteParam = 0x12,
    FaultFeedback = 0x15,
    SaveParams = 0x16,
}

impl TryFrom<u8> for CommType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::GetDeviceId),
            0x01 => Ok(Self::OperationCtrl),
            0x02 => Ok(Self::MotorFeedback),
            0x03 => Ok(Self::Enable),
            0x04 => Ok(Self::Stop),
            0x06 => Ok(Self::SetZero),
            0x07 => Ok(Self::SetCanId),
            0x11 => Ok(Self::ReadParam),
            0x12 => Ok(Self::WriteParam),
            0x15 => Ok(Self::FaultFeedback),
            0x16 => Ok(Self::SaveParams),
            _ => Err(()),
        }
    }
}
