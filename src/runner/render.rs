//! Small string assembly helpers for runner internals.

const WORKER_THREAD_PREFIX: &str = "ts-embed-vm-";
const BOOTSTRAP_MODULE_CONTEXT_TEMPLATE: &str =
    include_str!("../../assets/bootstrap_module_context.js");
const CALL_FUNCTION_TEMPLATE: &str = include_str!("../../assets/call_function.js");
#[cfg(feature = "async-promise")]
const ASYNC_CALL_FUNCTION_TEMPLATE: &str = include_str!("../../assets/async_call_function.js");
#[cfg(feature = "async-promise")]
const ASYNC_EMIT_EVENT_TEMPLATE: &str = include_str!("../../assets/async_emit_event.js");
const EMIT_EVENT_TEMPLATE: &str = include_str!("../../assets/emit_event.js");
const LIST_SUBSCRIPTIONS_TEMPLATE: &str = include_str!("../../assets/list_subscriptions.js");
const MODULE_ID_PLACEHOLDER: &str = "__MODULE_ID__";
const FUNCTION_NAME_PLACEHOLDER: &str = "__FUNCTION_NAME__";
const FUNCTION_ARGS_PLACEHOLDER: &str = "__FUNCTION_ARGS__";
const EVENT_NAME_PLACEHOLDER: &str = "__EVENT_NAME__";
const EVENT_PAYLOAD_PLACEHOLDER: &str = "__EVENT_PAYLOAD__";

pub(crate) fn eval_file_name(script_id: &str) -> String {
    suffixed(script_id, ".js")
}

pub(crate) fn worker_thread_name(worker_id: usize) -> String {
    prefixed_number(WORKER_THREAD_PREFIX, worker_id)
}

pub(crate) fn bootstrap_module_context_source() -> &'static str {
    BOOTSTRAP_MODULE_CONTEXT_TEMPLATE
}

pub(crate) fn function_call_source(
    module_id_json: &str,
    function_name_json: &str,
    args_json: &str,
) -> String {
    function_call_source_from_template(
        CALL_FUNCTION_TEMPLATE,
        module_id_json,
        function_name_json,
        args_json,
    )
}

#[cfg(feature = "async-promise")]
pub(crate) fn async_function_call_source(
    module_id_json: &str,
    function_name_json: &str,
    args_json: &str,
) -> String {
    function_call_source_from_template(
        ASYNC_CALL_FUNCTION_TEMPLATE,
        module_id_json,
        function_name_json,
        args_json,
    )
}

fn function_call_source_from_template(
    template: &str,
    module_id_json: &str,
    function_name_json: &str,
    args_json: &str,
) -> String {
    let with_module_id = template.replace(MODULE_ID_PLACEHOLDER, module_id_json);
    let with_function_name = with_module_id.replace(FUNCTION_NAME_PLACEHOLDER, function_name_json);
    with_function_name.replace(FUNCTION_ARGS_PLACEHOLDER, args_json)
}

pub(crate) fn emit_event_source(event_name_json: &str, payload_json: &str) -> String {
    event_source_from_template(EMIT_EVENT_TEMPLATE, event_name_json, payload_json)
}

#[cfg(feature = "async-promise")]
pub(crate) fn async_emit_event_source(event_name_json: &str, payload_json: &str) -> String {
    event_source_from_template(ASYNC_EMIT_EVENT_TEMPLATE, event_name_json, payload_json)
}

fn event_source_from_template(template: &str, event_name_json: &str, payload_json: &str) -> String {
    let with_event_name = template.replace(EVENT_NAME_PLACEHOLDER, event_name_json);
    with_event_name.replace(EVENT_PAYLOAD_PLACEHOLDER, payload_json)
}

pub(crate) fn list_subscriptions_source() -> &'static str {
    LIST_SUBSCRIPTIONS_TEMPLATE
}

fn suffixed(value: &str, suffix: &str) -> String {
    let mut output = String::with_capacity(value.len() + suffix.len());
    output.push_str(value);
    output.push_str(suffix);
    output
}

fn prefixed_number(prefix: &str, number: usize) -> String {
    let number_text = number.to_string();
    let mut output = String::with_capacity(prefix.len() + number_text.len());
    output.push_str(prefix);
    output.push_str(&number_text);
    output
}
