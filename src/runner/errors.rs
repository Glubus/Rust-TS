//! QuickJS error mapping helpers.

use std::borrow::Cow;

use crate::error::VmError;

use super::module_loader::{RUNTIME_MODULE_PREFIX, WorkerModuleStore};

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

/// Points the JavaScript locations of an execution error at the TypeScript source.
///
/// Each `rustts://graph/...:line:column` location, in the message or a stack frame,
/// that falls in a loaded script module becomes `path:line:column` in its TypeScript
/// file. Other locations stay as they are.
pub(crate) fn in_typescript(error: VmError, modules: &WorkerModuleStore) -> VmError {
    match error {
        VmError::Execution { details } => VmError::Execution {
            details: details
                .split_inclusive('\n')
                .map(|line| typescript_line(line, modules))
                .collect(),
        },
        error => error,
    }
}

/// One line of error details, with its runtime module location mapped when it has one.
fn typescript_line<'a>(line: &'a str, modules: &WorkerModuleStore) -> Cow<'a, str> {
    let Some(start) = line.find(RUNTIME_MODULE_PREFIX) else {
        return Cow::Borrowed(line);
    };
    let location = location_text(&line[start..]);
    match typescript_location(location, modules) {
        Some(typescript) => Cow::Owned(format!(
            "{}{typescript}{}",
            &line[..start],
            &line[start + location.len()..]
        )),
        None => Cow::Borrowed(line),
    }
}

/// The location `text` starts with: the rest of the line, without the `)` that closes a
/// stack frame.
fn location_text(text: &str) -> &str {
    let text = text.trim_end_matches(['\n', '\r']);
    text.strip_suffix(')').unwrap_or(text)
}

/// Maps `module id:line:column` to its TypeScript location.
fn typescript_location(location: &str, modules: &WorkerModuleStore) -> Option<String> {
    let (position, column) = location.rsplit_once(':')?;
    let (module_id, line) = position.rsplit_once(':')?;
    modules.locate(module_id, line.parse().ok()?, column.parse().ok()?)
}
