use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FaultClass {
    MlccShortLoad,
    VrmPhaseShort,
    FirmwareStateBad,
    GpuDieDead,
    HealthyStandby,
    UnknownAmbiguity,
}

impl FaultClass {
    pub fn to_code(&self) -> &str {
        match self {
            Self::MlccShortLoad => "ERR_MLCC_001",
            Self::VrmPhaseShort => "ERR_VRM_PHASE",
            Self::FirmwareStateBad => "ERR_FW_CRC_FAIL",
            Self::GpuDieDead => "ERR_SILICON_DEAD",
            Self::HealthyStandby => "STATUS_OK",
            Self::UnknownAmbiguity => "WARN_AMBIGUOUS",
        }
    }
}
