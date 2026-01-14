//! FHIR API implementation
//!
//! Provides `fhir.create`, `fhir.read`, `fhir.update`, `fhir.delete`, `fhir.search`
//! for FHIR operations from automations.
//!
//! Note: This module requires a FhirClient to be injected into the context before use.
//! The actual FHIR operations are delegated to the injected client.

use crate::apis::{get_arg_as_json, get_arg_as_string, json_to_js_value, make_exception};
use crate::bindings::*;
use crate::error::JscResult;
use std::collections::HashMap;
use std::ffi::CString;
use std::ptr;
use std::sync::Arc;
use tracing::{debug, warn};

/// FHIR client trait for automation operations
///
/// Implementations must be thread-safe and provide blocking operations.
pub trait FhirClient: Send + Sync {
    /// Create a new resource
    fn create(&self, resource: serde_json::Value) -> Result<serde_json::Value, String>;

    /// Read a resource by type and ID
    fn read(&self, resource_type: &str, id: &str) -> Result<serde_json::Value, String>;

    /// Update a resource (full replacement)
    fn update(&self, resource: serde_json::Value) -> Result<serde_json::Value, String>;

    /// Delete a resource
    fn delete(&self, resource_type: &str, id: &str) -> Result<(), String>;

    /// Search for resources
    fn search(
        &self,
        resource_type: &str,
        params: HashMap<String, String>,
    ) -> Result<serde_json::Value, String>;

    /// Patch a resource (partial update)
    fn patch(
        &self,
        resource_type: &str,
        id: &str,
        patch: serde_json::Value,
    ) -> Result<serde_json::Value, String>;
}

// Thread-local storage for FHIR client
// This is set before script evaluation and cleared after
thread_local! {
    static FHIR_CLIENT: std::cell::RefCell<Option<Arc<dyn FhirClient>>> = const { std::cell::RefCell::new(None) };
}

/// Set the FHIR client for the current thread
pub fn set_fhir_client(client: Arc<dyn FhirClient>) {
    FHIR_CLIENT.with(|c| {
        *c.borrow_mut() = Some(client);
    });
}

/// Clear the FHIR client for the current thread
pub fn clear_fhir_client() {
    FHIR_CLIENT.with(|c| {
        *c.borrow_mut() = None;
    });
}

/// Get the FHIR client for the current thread
fn get_fhir_client() -> Option<Arc<dyn FhirClient>> {
    FHIR_CLIENT.with(|c| c.borrow().clone())
}

/// Register the fhir API on the global object
pub fn register_fhir_api(ctx: JSContextRef) -> JscResult<()> {
    unsafe {
        // Create fhir object
        let fhir_obj = JSObjectMake(ctx, ptr::null_mut(), ptr::null_mut());

        // Register methods
        register_fhir_method(ctx, fhir_obj, "create", Some(js_fhir_create))?;
        register_fhir_method(ctx, fhir_obj, "read", Some(js_fhir_read))?;
        register_fhir_method(ctx, fhir_obj, "update", Some(js_fhir_update))?;
        register_fhir_method(ctx, fhir_obj, "delete", Some(js_fhir_delete))?;
        register_fhir_method(ctx, fhir_obj, "search", Some(js_fhir_search))?;
        register_fhir_method(ctx, fhir_obj, "patch", Some(js_fhir_patch))?;

        // Set fhir on global object
        let fhir_name = CString::new("fhir").unwrap();
        let fhir_name_ref = JSStringCreateWithUTF8CString(fhir_name.as_ptr());
        let global = JSContextGetGlobalObject(ctx);
        let mut exception: JSValueRef = ptr::null_mut();

        JSObjectSetProperty(
            ctx,
            global,
            fhir_name_ref,
            fhir_obj as JSValueRef,
            K_JS_PROPERTY_ATTRIBUTE_NONE,
            &mut exception,
        );

        JSStringRelease(fhir_name_ref);
    }

    Ok(())
}

unsafe fn register_fhir_method(
    ctx: JSContextRef,
    fhir_obj: JSObjectRef,
    name: &str,
    callback: JSObjectCallAsFunctionCallback,
) -> JscResult<()> {
    let name_cstr = CString::new(name).unwrap();
    let name_ref = JSStringCreateWithUTF8CString(name_cstr.as_ptr());

    let func = JSObjectMakeFunctionWithCallback(ctx, name_ref, callback);

    let mut exception: JSValueRef = ptr::null_mut();
    JSObjectSetProperty(
        ctx,
        fhir_obj,
        name_ref,
        func as JSValueRef,
        K_JS_PROPERTY_ATTRIBUTE_NONE,
        &mut exception,
    );

    JSStringRelease(name_ref);
    Ok(())
}

/// fhir.create(resource) - Create a new resource
///
/// ```javascript
/// const patient = fhir.create({
///     resourceType: "Patient",
///     name: [{ given: ["John"], family: "Doe" }]
/// });
/// console.log("Created patient:", patient.id);
/// ```
unsafe extern "C" fn js_fhir_create(
    ctx: JSContextRef,
    _function: JSObjectRef,
    _this_object: JSObjectRef,
    argument_count: usize,
    arguments: *const JSValueRef,
    exception: *mut JSValueRef,
) -> JSValueRef {
    let client = match get_fhir_client() {
        Some(c) => c,
        None => {
            *exception = make_exception(ctx, "FHIR client not available");
            return JSValueMakeUndefined(ctx);
        }
    };

    let resource = match get_arg_as_json(ctx, arguments, 0, argument_count) {
        Some(r) => r,
        None => {
            *exception = make_exception(ctx, "fhir.create requires a resource object");
            return JSValueMakeUndefined(ctx);
        }
    };

    debug!(
        resource_type = resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown"),
        "fhir.create called"
    );

    match client.create(resource) {
        Ok(result) => json_to_js_value(ctx, &result),
        Err(e) => {
            warn!(error = %e, "fhir.create failed");
            *exception = make_exception(ctx, &e);
            JSValueMakeUndefined(ctx)
        }
    }
}

/// fhir.read(resourceType, id) - Read a resource by ID
///
/// ```javascript
/// const patient = fhir.read("Patient", "123");
/// console.log("Patient name:", patient.name[0].family);
/// ```
unsafe extern "C" fn js_fhir_read(
    ctx: JSContextRef,
    _function: JSObjectRef,
    _this_object: JSObjectRef,
    argument_count: usize,
    arguments: *const JSValueRef,
    exception: *mut JSValueRef,
) -> JSValueRef {
    let client = match get_fhir_client() {
        Some(c) => c,
        None => {
            *exception = make_exception(ctx, "FHIR client not available");
            return JSValueMakeUndefined(ctx);
        }
    };

    let resource_type = match get_arg_as_string(ctx, arguments, 0, argument_count) {
        Some(t) => t,
        None => {
            *exception = make_exception(ctx, "fhir.read requires resourceType as first argument");
            return JSValueMakeUndefined(ctx);
        }
    };

    let id = match get_arg_as_string(ctx, arguments, 1, argument_count) {
        Some(i) => i,
        None => {
            *exception = make_exception(ctx, "fhir.read requires id as second argument");
            return JSValueMakeUndefined(ctx);
        }
    };

    debug!(resource_type = %resource_type, id = %id, "fhir.read called");

    match client.read(&resource_type, &id) {
        Ok(result) => json_to_js_value(ctx, &result),
        Err(e) => {
            warn!(error = %e, resource_type = %resource_type, id = %id, "fhir.read failed");
            *exception = make_exception(ctx, &e);
            JSValueMakeUndefined(ctx)
        }
    }
}

/// fhir.update(resource) - Update a resource (full replacement)
///
/// ```javascript
/// const patient = fhir.read("Patient", "123");
/// patient.name[0].family = "Smith";
/// const updated = fhir.update(patient);
/// ```
unsafe extern "C" fn js_fhir_update(
    ctx: JSContextRef,
    _function: JSObjectRef,
    _this_object: JSObjectRef,
    argument_count: usize,
    arguments: *const JSValueRef,
    exception: *mut JSValueRef,
) -> JSValueRef {
    let client = match get_fhir_client() {
        Some(c) => c,
        None => {
            *exception = make_exception(ctx, "FHIR client not available");
            return JSValueMakeUndefined(ctx);
        }
    };

    let resource = match get_arg_as_json(ctx, arguments, 0, argument_count) {
        Some(r) => r,
        None => {
            *exception = make_exception(ctx, "fhir.update requires a resource object");
            return JSValueMakeUndefined(ctx);
        }
    };

    debug!(
        resource_type = resource
            .get("resourceType")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown"),
        id = resource
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown"),
        "fhir.update called"
    );

    match client.update(resource) {
        Ok(result) => json_to_js_value(ctx, &result),
        Err(e) => {
            warn!(error = %e, "fhir.update failed");
            *exception = make_exception(ctx, &e);
            JSValueMakeUndefined(ctx)
        }
    }
}

/// fhir.delete(resourceType, id) - Delete a resource
///
/// ```javascript
/// fhir.delete("Patient", "123");
/// console.log("Patient deleted");
/// ```
unsafe extern "C" fn js_fhir_delete(
    ctx: JSContextRef,
    _function: JSObjectRef,
    _this_object: JSObjectRef,
    argument_count: usize,
    arguments: *const JSValueRef,
    exception: *mut JSValueRef,
) -> JSValueRef {
    let client = match get_fhir_client() {
        Some(c) => c,
        None => {
            *exception = make_exception(ctx, "FHIR client not available");
            return JSValueMakeUndefined(ctx);
        }
    };

    let resource_type = match get_arg_as_string(ctx, arguments, 0, argument_count) {
        Some(t) => t,
        None => {
            *exception = make_exception(ctx, "fhir.delete requires resourceType as first argument");
            return JSValueMakeUndefined(ctx);
        }
    };

    let id = match get_arg_as_string(ctx, arguments, 1, argument_count) {
        Some(i) => i,
        None => {
            *exception = make_exception(ctx, "fhir.delete requires id as second argument");
            return JSValueMakeUndefined(ctx);
        }
    };

    debug!(resource_type = %resource_type, id = %id, "fhir.delete called");

    match client.delete(&resource_type, &id) {
        Ok(()) => JSValueMakeUndefined(ctx),
        Err(e) => {
            warn!(error = %e, resource_type = %resource_type, id = %id, "fhir.delete failed");
            *exception = make_exception(ctx, &e);
            JSValueMakeUndefined(ctx)
        }
    }
}

/// fhir.search(resourceType, params) - Search for resources
///
/// ```javascript
/// const bundle = fhir.search("Patient", {
///     name: "Smith",
///     birthdate: "ge1990-01-01"
/// });
/// console.log("Found", bundle.total, "patients");
/// for (const entry of bundle.entry || []) {
///     console.log(entry.resource.id);
/// }
/// ```
unsafe extern "C" fn js_fhir_search(
    ctx: JSContextRef,
    _function: JSObjectRef,
    _this_object: JSObjectRef,
    argument_count: usize,
    arguments: *const JSValueRef,
    exception: *mut JSValueRef,
) -> JSValueRef {
    let client = match get_fhir_client() {
        Some(c) => c,
        None => {
            *exception = make_exception(ctx, "FHIR client not available");
            return JSValueMakeUndefined(ctx);
        }
    };

    let resource_type = match get_arg_as_string(ctx, arguments, 0, argument_count) {
        Some(t) => t,
        None => {
            *exception = make_exception(ctx, "fhir.search requires resourceType as first argument");
            return JSValueMakeUndefined(ctx);
        }
    };

    // Parse search parameters from second argument
    let params: HashMap<String, String> = get_arg_as_json(ctx, arguments, 1, argument_count)
        .and_then(|v| {
            v.as_object().map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
        })
        .unwrap_or_default();

    debug!(resource_type = %resource_type, params = ?params, "fhir.search called");

    match client.search(&resource_type, params) {
        Ok(result) => json_to_js_value(ctx, &result),
        Err(e) => {
            warn!(error = %e, resource_type = %resource_type, "fhir.search failed");
            *exception = make_exception(ctx, &e);
            JSValueMakeUndefined(ctx)
        }
    }
}

/// fhir.patch(resourceType, id, patch) - Patch a resource (partial update)
///
/// ```javascript
/// const updated = fhir.patch("Patient", "123", {
///     name: [{ given: ["Jane"], family: "Doe" }]
/// });
/// ```
unsafe extern "C" fn js_fhir_patch(
    ctx: JSContextRef,
    _function: JSObjectRef,
    _this_object: JSObjectRef,
    argument_count: usize,
    arguments: *const JSValueRef,
    exception: *mut JSValueRef,
) -> JSValueRef {
    let client = match get_fhir_client() {
        Some(c) => c,
        None => {
            *exception = make_exception(ctx, "FHIR client not available");
            return JSValueMakeUndefined(ctx);
        }
    };

    let resource_type = match get_arg_as_string(ctx, arguments, 0, argument_count) {
        Some(t) => t,
        None => {
            *exception = make_exception(ctx, "fhir.patch requires resourceType as first argument");
            return JSValueMakeUndefined(ctx);
        }
    };

    let id = match get_arg_as_string(ctx, arguments, 1, argument_count) {
        Some(i) => i,
        None => {
            *exception = make_exception(ctx, "fhir.patch requires id as second argument");
            return JSValueMakeUndefined(ctx);
        }
    };

    let patch = match get_arg_as_json(ctx, arguments, 2, argument_count) {
        Some(p) => p,
        None => {
            *exception = make_exception(ctx, "fhir.patch requires patch object as third argument");
            return JSValueMakeUndefined(ctx);
        }
    };

    debug!(resource_type = %resource_type, id = %id, "fhir.patch called");

    match client.patch(&resource_type, &id, patch) {
        Ok(result) => json_to_js_value(ctx, &result),
        Err(e) => {
            warn!(error = %e, resource_type = %resource_type, id = %id, "fhir.patch failed");
            *exception = make_exception(ctx, &e);
            JSValueMakeUndefined(ctx)
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests require JSC and a mock FHIR client, run in integration tests
}
