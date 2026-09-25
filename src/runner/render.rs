//! Small string assembly helpers for runner internals.

const WORKER_THREAD_PREFIX: &str = "rustts-";
const BOOTSTRAP_MODULE_CONTEXT_TEMPLATE: &str =
    include_str!("../../assets/bootstrap_module_context.js");
const HOST_LAZY_BINDINGS_TEMPLATE: &str = include_str!("../../assets/host_lazy_bindings.js");
const CALL_FUNCTION_TEMPLATE: &str = include_str!("../../assets/call_function.js");
#[cfg(feature = "async-promise")]
const ASYNC_EMIT_EVENT_TEMPLATE: &str = include_str!("../../assets/async_emit_event.js");
const EMIT_EVENT_TEMPLATE: &str = include_str!("../../assets/emit_event.js");
const LIST_SUBSCRIPTIONS_TEMPLATE: &str = include_str!("../../assets/list_subscriptions.js");
const MODULE_ID_PLACEHOLDER: &str = "__MODULE_ID__";
const FUNCTION_NAME_PLACEHOLDER: &str = "__FUNCTION_NAME__";
const FUNCTION_ARGS_PLACEHOLDER: &str = "__FUNCTION_ARGS__";
const EVENT_NAME_PLACEHOLDER: &str = "__EVENT_NAME__";
const EVENT_PAYLOAD_PLACEHOLDER: &str = "__EVENT_PAYLOAD__";
const CONTRACTS_PLACEHOLDER: &str = "__contracts__";

pub(crate) fn eval_file_name(script_id: &str) -> String {
    suffixed(script_id, ".js")
}

pub(crate) fn worker_thread_name(worker_id: usize) -> String {
    prefixed_number(WORKER_THREAD_PREFIX, worker_id)
}

/// Options for evaluating runner-owned global (non-module) scripts named `name.js`.
pub(crate) fn global_eval_options(name: &str) -> rquickjs::context::EvalOptions {
    let mut options = rquickjs::context::EvalOptions::default();
    options.global = true;
    options.strict = true;
    options.filename = Some(eval_file_name(name));
    options
}

pub(crate) fn bootstrap_module_context_source() -> &'static str {
    BOOTSTRAP_MODULE_CONTEXT_TEMPLATE
}

/// `contracts_json` is a JSON array of `[contract name, returns a Promise]` pairs.
pub(crate) fn host_lazy_bindings_source(contracts_json: &str) -> String {
    HOST_LAZY_BINDINGS_TEMPLATE.replace(CONTRACTS_PLACEHOLDER, contracts_json)
}

/// Calls one export and settles its result, so async exports resolve before returning.
pub(crate) fn function_call_source(
    module_id_json: &str,
    function_name_json: &str,
    args_json: &str,
) -> String {
    CALL_FUNCTION_TEMPLATE
        .replace(MODULE_ID_PLACEHOLDER, module_id_json)
        .replace(FUNCTION_NAME_PLACEHOLDER, function_name_json)
        .replace(FUNCTION_ARGS_PLACEHOLDER, args_json)
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
