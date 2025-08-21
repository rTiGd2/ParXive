pub mod progress;
pub mod rs_codec;
pub mod cuda_backend;
pub mod manifest;
pub mod volume;

// ---- i18n scaffolding (we'll plug proper Fluent in step 2) ----

/// Return localized strings from a message code + args
pub trait Localizer: Send + Sync {
    fn msg(&self, code: &'static str, args: &[(&'static str, String)]) -> String;
}

/// A no-op localizer that simply echoes message codes (temporary)
pub struct NoopLoc;
impl Localizer for NoopLoc {
    fn msg(&self, code: &'static str, _args: &[(&'static str, String)]) -> String {
        code.to_string()
    }
}

/// Message codes we'll gradually route through i18n
pub mod i18n_codes {
    pub const CREATE_START: &str   = "create-start";
    pub const PARITY_SUMMARY: &str = "parity-summary";
    pub const STRIPE_PROGRESS: &str= "stripe-progress";
    pub const VERIFY_RESULT: &str  = "verify-result";
    pub const REPAIR_SUMMARY: &str = "repair-summary";
}

