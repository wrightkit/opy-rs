//! QuickJS-NG embedding via the `libquickjs-ng-sys` FFI crate.
//!
//! QuickJS-NG is the engine family the OverPy reference runs on (the
//! quickjs-ng wasm build), so observable language behavior — completion
//! values, `typeof`, error messages such as `"interrupted"` or
//! `"out of memory"` — stays aligned with the reference.
//!
//! # Safety
//!
//! This module owns the raw `JSRuntime`/`JSContext` pointers and never shares
//! them across threads. The interrupt handler and the `console.log` native
//! function are armed only while the engine is at a stable address inside
//! [`evaluate`](JsEngine::evaluate), and they read engine-owned state through
//! raw pointers that stay valid until the context is freed in `Drop`.

use std::ffi::{CStr, CString};
use std::os::raw::{c_int, c_void};
use std::ptr;
use std::time::Instant;

use libquickjs_ng_sys as q;

use super::{Completion, EngineError, JsEngine};
use crate::limits::Limits;

pub(crate) struct QuickJsEngine {
    runtime: *mut q::JSRuntime,
    context: *mut q::JSContext,
    interrupt_deadline: Option<Instant>,
    console_lines: Vec<String>,
}

impl JsEngine for QuickJsEngine {
    fn new(limits: &Limits) -> Result<Self, EngineError> {
        unsafe {
            let runtime = q::JS_NewRuntime();
            if runtime.is_null() {
                return Err(EngineError::Internal("JS_NewRuntime failed".into()));
            }
            q::JS_SetMemoryLimit(runtime, limits.memory_limit_bytes);
            q::JS_SetMaxStackSize(runtime, limits.max_stack_bytes);
            let context = q::JS_NewContext(runtime);
            if context.is_null() {
                q::JS_FreeRuntime(runtime);
                return Err(EngineError::Internal("JS_NewContext failed".into()));
            }
            Ok(Self {
                runtime,
                context,
                interrupt_deadline: None,
                console_lines: Vec::new(),
            })
        }
    }

    fn install_console(&mut self) -> Result<(), EngineError> {
        unsafe {
            let sink_value = q::JS_Ext_NewPointer(
                q::JS_TAG_UNDEFINED,
                &mut self.console_lines as *mut Vec<String> as *mut c_void,
            );
            // `JS_NewCFunctionData2` duplicates the captured values, so the
            // function object owns a ref that dies with the context.
            let log_fn = q::JS_NewCFunctionData2(
                self.context,
                Some(console_log),
                c"log".as_ptr(),
                0, // length: variadic
                0, // magic: unused
                1, // data_len
                &sink_value as *const q::JSValue as *mut q::JSValue,
            );
            q::JS_FreeValue(self.context, sink_value);
            if q::JS_Ext_IsException(log_fn) {
                return Err(self.internal_from_pending("JS_NewCFunctionData2 failed"));
            }
            let console = q::JS_NewObject(self.context);
            if q::JS_Ext_IsException(console) {
                q::JS_FreeValue(self.context, log_fn);
                return Err(self.internal_from_pending("JS_NewObject failed"));
            }
            // `JS_SetPropertyStr` takes ownership of the value.
            q::JS_SetPropertyStr(self.context, console, c"log".as_ptr(), log_fn);
            let global = q::JS_GetGlobalObject(self.context);
            if q::JS_Ext_IsException(global) {
                q::JS_FreeValue(self.context, console);
                return Err(self.internal_from_pending("JS_GetGlobalObject failed"));
            }
            q::JS_SetPropertyStr(self.context, global, c"console".as_ptr(), console);
            q::JS_FreeValue(self.context, global);
        }
        Ok(())
    }

    fn evaluate(&mut self, source: &str, filename: &str) -> Result<Completion, EngineError> {
        let source = CString::new(source)
            .map_err(|_| EngineError::Internal("script contains a NUL byte".into()))?;
        let filename = CString::new(filename)
            .map_err(|_| EngineError::Internal("script name contains a NUL byte".into()))?;
        unsafe {
            // Anchor the stack-size measurement at this entry point so the JS
            // stack limit is measured from here rather than from wherever the
            // runtime was created (the reference wrapper does the same).
            q::JS_UpdateStackTop(self.runtime);
            let value = q::JS_Eval(
                self.context,
                source.as_ptr(),
                source.as_bytes().len(),
                filename.as_ptr(),
                q::JS_EVAL_TYPE_GLOBAL as c_int,
            );
            if q::JS_Ext_IsException(value) {
                let exception = q::JS_GetException(self.context);
                // Under memory exhaustion QuickJS-NG reports the exception as
                // null on some platforms (Linux CI observed "null" rendered
                // from a null exception). Map a null exception to the stable
                // out-of-memory message so resource-limit behavior is
                // platform-independent.
                let message = if q::JS_Ext_IsNull(exception) {
                    "out of memory".to_string()
                } else {
                    self.prop_string_or(exception, c"message", self.value_string(exception))
                };
                let stack = self.prop_string_or(exception, c"stack", String::new());
                q::JS_FreeValue(self.context, exception);
                return Err(EngineError::Exception { message, stack });
            }
            if q::JS_Ext_IsString(value) {
                let raw = q::JS_ToCStringLen2(self.context, ptr::null_mut(), value, true);
                let text = if raw.is_null() {
                    String::new()
                } else {
                    let s = CStr::from_ptr(raw).to_string_lossy().into_owned();
                    q::JS_FreeCString(self.context, raw);
                    s
                };
                q::JS_FreeValue(self.context, value);
                Ok(Completion::String(text))
            } else {
                let type_name = self.typeof_name(value);
                q::JS_FreeValue(self.context, value);
                Ok(Completion::NonString(type_name))
            }
        }
    }

    fn set_interrupt_deadline(&mut self, deadline: Option<Instant>) {
        self.interrupt_deadline = deadline;
        unsafe {
            q::JS_SetInterruptHandler(
                self.runtime,
                Some(interrupt_handler),
                &self.interrupt_deadline as *const Option<Instant> as *mut c_void,
            );
        }
    }

    fn console_output(&self) -> &[String] {
        &self.console_lines
    }
}

impl QuickJsEngine {
    /// Reads the string property `name` of `obj`; when the property is not a
    /// string (missing, or a thrown non-Error value), returns `fallback`.
    ///
    /// # Safety
    ///
    /// `obj` must be a live JSValue owned by `self.context`; the property is
    /// freed before returning.
    unsafe fn prop_string_or(&self, obj: q::JSValue, name: &CStr, fallback: String) -> String {
        unsafe {
            let value = q::JS_GetPropertyStr(self.context, obj, name.as_ptr());
            let is_string = q::JS_Ext_IsString(value);
            let text = if is_string {
                self.value_string(value)
            } else {
                fallback
            };
            q::JS_FreeValue(self.context, value);
            text
        }
    }

    /// Renders `value` with `String(value)` semantics.
    ///
    /// # Safety
    ///
    /// `value` must be a live JSValue owned by `self.context`.
    unsafe fn value_string(&self, value: q::JSValue) -> String {
        unsafe {
            let raw = q::JS_ToCStringLen2(self.context, ptr::null_mut(), value, true);
            if raw.is_null() {
                // The conversion threw (hostile `toString`): clear the pending
                // exception so the engine stays usable.
                if q::JS_HasException(self.context) {
                    let exception = q::JS_GetException(self.context);
                    q::JS_FreeValue(self.context, exception);
                }
                return String::from("[unserializable]");
            }
            let text = CStr::from_ptr(raw).to_string_lossy().into_owned();
            q::JS_FreeCString(self.context, raw);
            text
        }
    }

    /// The ECMAScript `typeof` name of `value`, matching the reference's
    /// `context.typeof(...)` behavior.
    ///
    /// # Safety
    ///
    /// `value` must be a live JSValue owned by `self.context`.
    unsafe fn typeof_name(&self, value: q::JSValue) -> &'static str {
        unsafe {
            if q::JS_Ext_IsUndefined(value) {
                "undefined"
            } else if q::JS_Ext_IsNull(value) {
                "object"
            } else if q::JS_Ext_IsBool(value) {
                "boolean"
            } else if q::JS_Ext_IsString(value) {
                "string"
            } else if q::JS_Ext_IsSymbol(value) {
                "symbol"
            } else if q::JS_Ext_IsNumber(value) {
                "number"
            } else if q::JS_Ext_IsBigInt(value) {
                "bigint"
            } else if q::JS_IsFunction(self.context, value) {
                "function"
            } else {
                "object"
            }
        }
    }

    /// Converts a pending engine exception into an internal error and clears
    /// it from the context.
    ///
    /// # Safety
    ///
    /// Must only be called right after a QuickJS call failed with a pending
    /// exception on `self.context`.
    unsafe fn internal_from_pending(&self, context: &str) -> EngineError {
        let message = unsafe {
            if q::JS_HasException(self.context) {
                let exception = q::JS_GetException(self.context);
                let text = self.value_string(exception);
                q::JS_FreeValue(self.context, exception);
                text
            } else {
                String::from("no pending exception")
            }
        };
        EngineError::Internal(format!("{context}: {message}"))
    }
}

impl Drop for QuickJsEngine {
    fn drop(&mut self) {
        unsafe {
            q::JS_SetInterruptHandler(self.runtime, None, ptr::null_mut());
            q::JS_FreeContext(self.context);
            q::JS_FreeRuntime(self.runtime);
        }
    }
}

/// Deadline-based interrupt handler: returns non-zero once the deadline set by
/// [`JsEngine::set_interrupt_deadline`] has passed, aborting the running
/// script with the QuickJS `"interrupted"` error.
///
/// # Safety
///
/// `opaque` must point to the engine's `interrupt_deadline` field; the handler
/// is only invoked while the engine is alive (during JS execution).
unsafe extern "C" fn interrupt_handler(_runtime: *mut q::JSRuntime, opaque: *mut c_void) -> c_int {
    let deadline = unsafe { &*(opaque as *const Option<Instant>) };
    match deadline {
        Some(deadline) if Instant::now() >= *deadline => 1,
        _ => 0,
    }
}

/// Native `console.log`: renders each argument with `String()` semantics,
/// joins them on `" "`, and appends the line to the engine's output buffer.
///
/// # Safety
///
/// The captured data value must be the pointer value created in
/// [`JsEngine::install_console`], pointing at the engine's `console_lines`
/// field; the engine is alive for the whole call.
unsafe extern "C" fn console_log(
    context: *mut q::JSContext,
    _this: q::JSValue,
    argc: c_int,
    argv: *mut q::JSValue,
    _magic: c_int,
    func_data: *mut q::JSValue,
) -> q::JSValue {
    let sink = unsafe { &mut *((*func_data).u.ptr as *mut Vec<String>) };
    let mut rendered = Vec::with_capacity(argc as usize);
    for i in 0..argc {
        let arg = unsafe { *argv.add(i as usize) };
        rendered.push(unsafe { stringify_arg(context, arg) });
    }
    sink.push(rendered.join(" "));
    unsafe { q::JS_Ext_NewSpecialValue(q::JS_TAG_UNDEFINED, 0) }
}

/// Renders one `console.log` argument with `String()` semantics, falling back
/// to `"[unserializable]"` when the conversion throws (matching the
/// reference's `try { String(context.dump(arg)) } catch` fallback).
///
/// # Safety
///
/// `arg` must be a live JSValue owned by `context`.
unsafe fn stringify_arg(context: *mut q::JSContext, arg: q::JSValue) -> String {
    let raw = unsafe { q::JS_ToCStringLen2(context, ptr::null_mut(), arg, true) };
    if raw.is_null() {
        if unsafe { q::JS_HasException(context) } {
            let exception = unsafe { q::JS_GetException(context) };
            unsafe { q::JS_FreeValue(context, exception) };
        }
        return String::from("[unserializable]");
    }
    unsafe {
        let s = CStr::from_ptr(raw).to_string_lossy().into_owned();
        q::JS_FreeCString(context, raw);
        s
    }
}
