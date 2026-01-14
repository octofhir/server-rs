//! Console API implementation
//!
//! Provides `console.log`, `console.warn`, `console.error`, `console.debug`, and `console.info`
//! that route output to the tracing crate.

use crate::bindings::*;
use crate::error::JscResult;
use crate::value::js_string_to_rust;
use std::ffi::CString;
use std::ptr;
use tracing::{debug, error, info, warn};

/// Register the console API on the global object
pub fn register_console_api(ctx: JSContextRef) -> JscResult<()> {
    unsafe {
        // Create console object
        let console_obj = JSObjectMake(ctx, ptr::null_mut(), ptr::null_mut());

        // Register console methods
        register_console_method(ctx, console_obj, "log", Some(js_console_log))?;
        register_console_method(ctx, console_obj, "info", Some(js_console_info))?;
        register_console_method(ctx, console_obj, "debug", Some(js_console_debug))?;
        register_console_method(ctx, console_obj, "warn", Some(js_console_warn))?;
        register_console_method(ctx, console_obj, "error", Some(js_console_error))?;

        // Set console on global object
        let console_name = CString::new("console").unwrap();
        let console_name_ref = JSStringCreateWithUTF8CString(console_name.as_ptr());
        let global = JSContextGetGlobalObject(ctx);
        let mut exception: JSValueRef = ptr::null_mut();

        JSObjectSetProperty(
            ctx,
            global,
            console_name_ref,
            console_obj as JSValueRef,
            K_JS_PROPERTY_ATTRIBUTE_NONE,
            &mut exception,
        );

        JSStringRelease(console_name_ref);
    }

    Ok(())
}

unsafe fn register_console_method(
    ctx: JSContextRef,
    console_obj: JSObjectRef,
    name: &str,
    callback: JSObjectCallAsFunctionCallback,
) -> JscResult<()> {
    let name_cstr = CString::new(name).unwrap();
    let name_ref = JSStringCreateWithUTF8CString(name_cstr.as_ptr());

    let func = JSObjectMakeFunctionWithCallback(ctx, name_ref, callback);

    let mut exception: JSValueRef = ptr::null_mut();
    JSObjectSetProperty(
        ctx,
        console_obj,
        name_ref,
        func as JSValueRef,
        K_JS_PROPERTY_ATTRIBUTE_NONE,
        &mut exception,
    );

    JSStringRelease(name_ref);
    Ok(())
}

/// Format arguments into a single string for logging
unsafe fn format_console_args(
    ctx: JSContextRef,
    argument_count: usize,
    arguments: *const JSValueRef,
) -> String {
    let mut parts = Vec::with_capacity(argument_count);

    for i in 0..argument_count {
        let value = *arguments.add(i);
        if value.is_null() {
            parts.push("null".to_string());
            continue;
        }

        // Try to convert to string
        let mut exception: JSValueRef = ptr::null_mut();

        // For objects/arrays, use JSON.stringify
        if JSValueIsObject(ctx, value) && !JSValueIsNull(ctx, value) {
            let json_str = JSValueCreateJSONString(ctx, value, 0, &mut exception);
            if !json_str.is_null() {
                let s = js_string_to_rust(json_str);
                JSStringRelease(json_str);
                parts.push(s);
                continue;
            }
        }

        // Default to string conversion
        let js_str = JSValueToStringCopy(ctx, value, &mut exception);
        if !js_str.is_null() {
            let s = js_string_to_rust(js_str);
            JSStringRelease(js_str);
            parts.push(s);
        } else {
            parts.push("[object]".to_string());
        }
    }

    parts.join(" ")
}

/// console.log implementation
unsafe extern "C" fn js_console_log(
    ctx: JSContextRef,
    _function: JSObjectRef,
    _this_object: JSObjectRef,
    argument_count: usize,
    arguments: *const JSValueRef,
    _exception: *mut JSValueRef,
) -> JSValueRef {
    let message = format_console_args(ctx, argument_count, arguments);
    info!(target: "automation", "{}", message);
    JSValueMakeUndefined(ctx)
}

/// console.info implementation
unsafe extern "C" fn js_console_info(
    ctx: JSContextRef,
    _function: JSObjectRef,
    _this_object: JSObjectRef,
    argument_count: usize,
    arguments: *const JSValueRef,
    _exception: *mut JSValueRef,
) -> JSValueRef {
    let message = format_console_args(ctx, argument_count, arguments);
    info!(target: "automation", "{}", message);
    JSValueMakeUndefined(ctx)
}

/// console.debug implementation
unsafe extern "C" fn js_console_debug(
    ctx: JSContextRef,
    _function: JSObjectRef,
    _this_object: JSObjectRef,
    argument_count: usize,
    arguments: *const JSValueRef,
    _exception: *mut JSValueRef,
) -> JSValueRef {
    let message = format_console_args(ctx, argument_count, arguments);
    debug!(target: "automation", "{}", message);
    JSValueMakeUndefined(ctx)
}

/// console.warn implementation
unsafe extern "C" fn js_console_warn(
    ctx: JSContextRef,
    _function: JSObjectRef,
    _this_object: JSObjectRef,
    argument_count: usize,
    arguments: *const JSValueRef,
    _exception: *mut JSValueRef,
) -> JSValueRef {
    let message = format_console_args(ctx, argument_count, arguments);
    warn!(target: "automation", "{}", message);
    JSValueMakeUndefined(ctx)
}

/// console.error implementation
unsafe extern "C" fn js_console_error(
    ctx: JSContextRef,
    _function: JSObjectRef,
    _this_object: JSObjectRef,
    argument_count: usize,
    arguments: *const JSValueRef,
    _exception: *mut JSValueRef,
) -> JSValueRef {
    let message = format_console_args(ctx, argument_count, arguments);
    error!(target: "automation", "{}", message);
    JSValueMakeUndefined(ctx)
}

#[cfg(test)]
mod tests {
    // Tests require JSC to be available, run in integration tests
}
