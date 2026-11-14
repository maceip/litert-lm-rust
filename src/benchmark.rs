//! Benchmark information from sessions / conversations.

use std::ptr::NonNull;

use crate::error::{check, Result};
use crate::ffi;

/// Timing and throughput metrics from a session or conversation.
pub struct BenchmarkInfo {
    pub(crate) raw: NonNull<ffi::LiteRtLmBenchmarkInfo>,
}

impl BenchmarkInfo {
    pub fn time_to_first_token(&self) -> Result<f64> {
        let mut out = 0.0;
        check(
            "litert_lm_benchmark_info_get_time_to_first_token",
            unsafe {
                ffi::litert_lm_benchmark_info_get_time_to_first_token(self.raw.as_ptr(), &mut out)
            },
        )?;
        Ok(out)
    }

    pub fn total_init_time_secs(&self) -> Result<f64> {
        let mut out = 0.0;
        check(
            "litert_lm_benchmark_info_get_total_init_time_in_second",
            unsafe {
                ffi::litert_lm_benchmark_info_get_total_init_time_in_second(
                    self.raw.as_ptr(),
                    &mut out,
                )
            },
        )?;
        Ok(out)
    }

    pub fn num_prefill_turns(&self) -> Result<usize> {
        let mut out = 0;
        check(
            "litert_lm_benchmark_info_get_num_prefill_turns",
            unsafe {
                ffi::litert_lm_benchmark_info_get_num_prefill_turns(self.raw.as_ptr(), &mut out)
            },
        )?;
        Ok(out)
    }

    pub fn num_decode_turns(&self) -> Result<usize> {
        let mut out = 0;
        check(
            "litert_lm_benchmark_info_get_num_decode_turns",
            unsafe {
                ffi::litert_lm_benchmark_info_get_num_decode_turns(self.raw.as_ptr(), &mut out)
            },
        )?;
        Ok(out)
    }

    pub fn prefill_token_count_at(&self, index: usize) -> Result<usize> {
        let mut out = 0;
        check(
            "litert_lm_benchmark_info_get_prefill_token_count_at",
            unsafe {
                ffi::litert_lm_benchmark_info_get_prefill_token_count_at(
                    self.raw.as_ptr(),
                    index,
                    &mut out,
                )
            },
        )?;
        Ok(out)
    }

    pub fn decode_token_count_at(&self, index: usize) -> Result<usize> {
        let mut out = 0;
        check(
            "litert_lm_benchmark_info_get_decode_token_count_at",
            unsafe {
                ffi::litert_lm_benchmark_info_get_decode_token_count_at(
                    self.raw.as_ptr(),
                    index,
                    &mut out,
                )
            },
        )?;
        Ok(out)
    }

    pub fn prefill_tokens_per_sec_at(&self, index: usize) -> Result<f64> {
        let mut out = 0.0;
        check(
            "litert_lm_benchmark_info_get_prefill_tokens_per_sec_at",
            unsafe {
                ffi::litert_lm_benchmark_info_get_prefill_tokens_per_sec_at(
                    self.raw.as_ptr(),
                    index,
                    &mut out,
                )
            },
        )?;
        Ok(out)
    }

    pub fn decode_tokens_per_sec_at(&self, index: usize) -> Result<f64> {
        let mut out = 0.0;
        check(
            "litert_lm_benchmark_info_get_decode_tokens_per_sec_at",
            unsafe {
                ffi::litert_lm_benchmark_info_get_decode_tokens_per_sec_at(
                    self.raw.as_ptr(),
                    index,
                    &mut out,
                )
            },
        )?;
        Ok(out)
    }

    /// Maximum number of tokens the session was configured for.
    pub fn max_num_tokens(&self) -> Result<i32> {
        let mut out = 0;
        check(
            "litert_lm_benchmark_info_get_max_num_tokens",
            unsafe {
                ffi::litert_lm_benchmark_info_get_max_num_tokens(self.raw.as_ptr(), &mut out)
            },
        )?;
        Ok(out)
    }
}

impl Drop for BenchmarkInfo {
    fn drop(&mut self) {
        unsafe { ffi::litert_lm_benchmark_info_delete(self.raw.as_ptr()) };
    }
}

impl std::fmt::Debug for BenchmarkInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BenchmarkInfo")
            .field("time_to_first_token", &self.time_to_first_token())
            .field("total_init_time_secs", &self.total_init_time_secs())
            .field("num_prefill_turns", &self.num_prefill_turns())
            .field("num_decode_turns", &self.num_decode_turns())
            .finish()
    }
}
