//! Optional CUDA backend (feature `cuda`). CPU fallback is default.

#[cfg(feature = "cuda")]
pub mod cuda {
    use anyhow::Result;
    use rustacuda::prelude::*;
    use std::ffi::CString;

    // Minimal PTX stub; swap for a real RS kernel later.
    const PTX: &str = r#"
.version 6.0
.target sm_50
.address_size 64

.visible .entry noop_kernel(
    .param .u64 pdata,
    .param .u32 len
)
{
    ret;
}
"#;

    pub struct CudaCtx {
        _context: Context, // keep context alive
        module: Module,
    }

    impl CudaCtx {
        pub fn new() -> Result<Self> {
            rustacuda::init(CudaFlags::empty())?;
            let device = Device::get_device(0)?;
            let context = Context::create_and_push(
                ContextFlags::MAP_HOST | ContextFlags::SCHED_AUTO,
                device,
            )?;
            let module = Module::load_from_string(&CString::new(PTX).unwrap())?;
            Ok(Self { _context: context, module })
        }

        /// Sanity-check kernel launch (no-op). Replace with real encode later.
        pub fn encode_noop(&self) -> Result<()> {
            let func = self.module.get_function(&CString::new("noop_kernel").unwrap())?;

            // launch! requires the stream to be a local identifier
            let stream = Stream::new(StreamFlags::DEFAULT, None)?;

            unsafe {
                rustacuda::launch!(func<<<1, 1, 0, stream>>>(0u64, 0u32))?;
            }
            stream.synchronize()?;
            Ok(())
        }
    }
}

#[cfg(not(feature = "cuda"))]
pub mod cuda {
    use anyhow::Result;
    pub struct CudaCtx;
    impl CudaCtx {
        pub fn new() -> Result<Self> {
            Ok(CudaCtx)
        }
        pub fn encode_noop(&self) -> Result<()> {
            Ok(())
        }
    }
}
