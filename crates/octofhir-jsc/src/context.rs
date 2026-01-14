//! JSC Context wrapper with safe evaluation and object management

use crate::bindings::*;
use crate::error::{JscError, JscResult};
use crate::value::{extract_exception, JscValue};
use std::collections::HashMap;
use std::ffi::CString;
use std::ptr;

/// A JavaScript execution context
///
/// Wraps a JSGlobalContext and provides safe methods for script evaluation
/// and object manipulation.
pub struct JscContext {
    ctx: JSGlobalContextRef,
    /// Registered native functions (for future Rust-side callback management)
    #[allow(dead_code)]
    registered_functions: HashMap<String, Box<dyn Fn(Vec<JscValue>) -> JscResult<JscValue>>>,
}

impl JscContext {
    /// Create a new JavaScript context
    pub fn new() -> JscResult<Self> {
        unsafe {
            let ctx = JSGlobalContextCreate(ptr::null_mut());
            if ctx.is_null() {
                return Err(JscError::ContextCreation(
                    "JSGlobalContextCreate returned null".to_string(),
                ));
            }

            Ok(Self {
                ctx,
                registered_functions: HashMap::new(),
            })
        }
    }

    /// Get the raw context pointer
    pub fn raw(&self) -> JSContextRef {
        self.ctx as JSContextRef
    }

    /// Get the global object
    pub fn global_object(&self) -> JSObjectRef {
        unsafe { JSContextGetGlobalObject(self.ctx as JSContextRef) }
    }

    /// Evaluate a JavaScript script and return the result
    pub fn eval(&self, script: &str) -> JscResult<JscValue> {
        self.eval_with_source(script, "<eval>")
    }

    /// Evaluate a JavaScript script with source URL (for better error messages)
    pub fn eval_with_source(&self, script: &str, source_url: &str) -> JscResult<JscValue> {
        let script_cstr = CString::new(script)
            .map_err(|e| JscError::Internal(format!("Invalid script: {}", e)))?;
        let source_cstr = CString::new(source_url)
            .map_err(|e| JscError::Internal(format!("Invalid source URL: {}", e)))?;

        unsafe {
            let script_ref = JSStringCreateWithUTF8CString(script_cstr.as_ptr());
            let source_ref = JSStringCreateWithUTF8CString(source_cstr.as_ptr());
            let mut exception: JSValueRef = ptr::null_mut();

            let result = JSEvaluateScript(
                self.ctx as JSContextRef,
                script_ref,
                ptr::null_mut(), // this object
                source_ref,
                1, // starting line number
                &mut exception,
            );

            JSStringRelease(script_ref);
            JSStringRelease(source_ref);

            if !exception.is_null() {
                return Err(extract_exception(self.ctx as JSContextRef, exception));
            }

            Ok(JscValue::new(self.ctx as JSContextRef, result))
        }
    }

    /// Set a property on the global object
    pub fn set_global(&self, name: &str, value: &JscValue) -> JscResult<()> {
        let name_cstr =
            CString::new(name).map_err(|e| JscError::Internal(format!("Invalid name: {}", e)))?;

        unsafe {
            let name_ref = JSStringCreateWithUTF8CString(name_cstr.as_ptr());
            let mut exception: JSValueRef = ptr::null_mut();

            JSObjectSetProperty(
                self.ctx as JSContextRef,
                self.global_object(),
                name_ref,
                value.raw(),
                K_JS_PROPERTY_ATTRIBUTE_NONE,
                &mut exception,
            );

            JSStringRelease(name_ref);

            if !exception.is_null() {
                return Err(extract_exception(self.ctx as JSContextRef, exception));
            }

            Ok(())
        }
    }

    /// Get a property from the global object
    pub fn get_global(&self, name: &str) -> JscResult<JscValue> {
        let name_cstr =
            CString::new(name).map_err(|e| JscError::Internal(format!("Invalid name: {}", e)))?;

        unsafe {
            let name_ref = JSStringCreateWithUTF8CString(name_cstr.as_ptr());
            let mut exception: JSValueRef = ptr::null_mut();

            let value = JSObjectGetProperty(
                self.ctx as JSContextRef,
                self.global_object(),
                name_ref,
                &mut exception,
            );

            JSStringRelease(name_ref);

            if !exception.is_null() {
                return Err(extract_exception(self.ctx as JSContextRef, exception));
            }

            Ok(JscValue::new(self.ctx as JSContextRef, value))
        }
    }

    /// Create an empty JavaScript object
    pub fn create_object(&self) -> JscValue {
        unsafe {
            let obj = JSObjectMake(self.ctx as JSContextRef, ptr::null_mut(), ptr::null_mut());
            JscValue::new(self.ctx as JSContextRef, obj as JSValueRef)
        }
    }

    /// Set a property on an object
    pub fn set_property(&self, object: JSObjectRef, name: &str, value: &JscValue) -> JscResult<()> {
        let name_cstr =
            CString::new(name).map_err(|e| JscError::Internal(format!("Invalid name: {}", e)))?;

        unsafe {
            let name_ref = JSStringCreateWithUTF8CString(name_cstr.as_ptr());
            let mut exception: JSValueRef = ptr::null_mut();

            JSObjectSetProperty(
                self.ctx as JSContextRef,
                object,
                name_ref,
                value.raw(),
                K_JS_PROPERTY_ATTRIBUTE_NONE,
                &mut exception,
            );

            JSStringRelease(name_ref);

            if !exception.is_null() {
                return Err(extract_exception(self.ctx as JSContextRef, exception));
            }

            Ok(())
        }
    }

    /// Register a native function callback
    ///
    /// The callback will be exposed to JavaScript with the given name on the global object.
    pub fn register_function(
        &self,
        name: &str,
        callback: JSObjectCallAsFunctionCallback,
    ) -> JscResult<()> {
        let name_cstr =
            CString::new(name).map_err(|e| JscError::Internal(format!("Invalid name: {}", e)))?;

        unsafe {
            let name_ref = JSStringCreateWithUTF8CString(name_cstr.as_ptr());
            let func =
                JSObjectMakeFunctionWithCallback(self.ctx as JSContextRef, name_ref, callback);

            let mut exception: JSValueRef = ptr::null_mut();
            JSObjectSetProperty(
                self.ctx as JSContextRef,
                self.global_object(),
                name_ref,
                func as JSValueRef,
                K_JS_PROPERTY_ATTRIBUTE_NONE,
                &mut exception,
            );

            JSStringRelease(name_ref);

            if !exception.is_null() {
                return Err(extract_exception(self.ctx as JSContextRef, exception));
            }

            Ok(())
        }
    }

    /// Force garbage collection
    pub fn gc(&self) {
        unsafe {
            JSGarbageCollect(self.ctx as JSContextRef);
        }
    }

    /// Inject a JSON object as a global variable
    pub fn inject_json(&self, name: &str, json: &str) -> JscResult<()> {
        let value = JscValue::from_json(self.ctx as JSContextRef, json)?;
        self.set_global(name, &value)
    }

    /// Create a string value
    pub fn string(&self, s: &str) -> JscResult<JscValue> {
        JscValue::string(self.ctx as JSContextRef, s)
    }

    /// Create a number value
    pub fn number(&self, n: f64) -> JscValue {
        JscValue::number(self.ctx as JSContextRef, n)
    }

    /// Create a boolean value
    pub fn boolean(&self, b: bool) -> JscValue {
        JscValue::boolean(self.ctx as JSContextRef, b)
    }

    /// Create an undefined value
    pub fn undefined(&self) -> JscValue {
        JscValue::undefined(self.ctx as JSContextRef)
    }

    /// Create a null value
    pub fn null(&self) -> JscValue {
        JscValue::null(self.ctx as JSContextRef)
    }
}

impl Drop for JscContext {
    fn drop(&mut self) {
        unsafe {
            JSGlobalContextRelease(self.ctx);
        }
    }
}

// JscContext is not Send or Sync - JSC contexts must be used from a single thread
// This matches the JSC threading model

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_context() {
        let ctx = JscContext::new().unwrap();
        drop(ctx);
    }

    #[test]
    fn test_eval_simple() {
        let ctx = JscContext::new().unwrap();
        let result = ctx.eval("2 + 2").unwrap();
        assert_eq!(result.to_number().unwrap(), 4.0);
    }

    #[test]
    fn test_eval_string() {
        let ctx = JscContext::new().unwrap();
        let result = ctx.eval("'hello' + ' ' + 'world'").unwrap();
        assert_eq!(result.to_string().unwrap(), "hello world");
    }

    #[test]
    fn test_set_get_global() {
        let ctx = JscContext::new().unwrap();
        let value = ctx.number(42.0);
        ctx.set_global("myVar", &value).unwrap();

        let result = ctx.eval("myVar * 2").unwrap();
        assert_eq!(result.to_number().unwrap(), 84.0);
    }

    #[test]
    fn test_inject_json() {
        let ctx = JscContext::new().unwrap();
        ctx.inject_json("config", r#"{"name": "test", "value": 123}"#)
            .unwrap();

        let name = ctx.eval("config.name").unwrap();
        assert_eq!(name.to_string().unwrap(), "test");

        let value = ctx.eval("config.value").unwrap();
        assert_eq!(value.to_number().unwrap(), 123.0);
    }

    #[test]
    fn test_eval_error() {
        let ctx = JscContext::new().unwrap();
        let result = ctx.eval("throw new Error('test error')");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("test error"));
    }
}
