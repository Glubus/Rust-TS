//! QuickJS error mapping helpers.

use crate::error::VmError;

pub(crate) fn js_error(error: rquickjs::Error) -> VmError {
    VmError::Execution {
        details: error.to_string(),
    }
}

pub(crate) fn caught_js_error(error: rquickjs::CaughtError<'_>) -> VmError {
    VmError::Execution {
        details: caught_js_error_details(&error),
    }
}

pub(crate) fn caught_js_error_details(error: &rquickjs::CaughtError<'_>) -> String {
    match error {
        rquickjs::CaughtError::Exception(exception) => exception_details(exception),
        rquickjs::CaughtError::Value(value) => js_value_details(value),
        rquickjs::CaughtError::Error(error) => error.to_string(),
    }
}

/// Describes a thrown or rejected JavaScript value, with its stack when it is an `Error`.
pub(crate) fn js_value_details(value: &rquickjs::Value<'_>) -> String {
    match value
        .as_object()
        .and_then(|object| rquickjs::Exception::from_object(object.clone()))
    {
        Some(exception) => exception_details(&exception),
        None => format!("non-error exception: {value:?}"),
    }
}

fn exception_details(exception: &rquickjs::Exception<'_>) -> String {
    let message = exception
        .message()
        .unwrap_or_else(|| String::from("javascript exception"));
    match exception.stack() {
        Some(stack) if !stack.trim().is_empty() => format!("{message}\n{stack}"),
        _ => message,
    }
}
