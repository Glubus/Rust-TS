use std::collections::BTreeMap;

use crate::contract::{HostContractAbi, HostContractDescriptor};

#[derive(Debug, Clone)]
enum HostModuleBinding {
    Function { contract_name: String },
    Callback { event_name: String },
}

#[derive(Debug, Default)]
struct HostModuleNode {
    children: BTreeMap<String, HostModuleNode>,
    binding: Option<HostModuleBinding>,
}

/// Source of every host import module. Function exports are the native functions the
/// engine installs in `globalThis.__rustts_native`; callback exports register handlers.
pub(crate) fn render_host_import_modules(
    descriptors: &[HostContractDescriptor],
) -> BTreeMap<String, String> {
    let mut modules = BTreeMap::<String, HostModuleNode>::new();

    for descriptor in descriptors {
        let Some(binding) = binding_for_descriptor(descriptor) else {
            continue;
        };
        insert_binding(
            modules.entry(descriptor.import.module.clone()).or_default(),
            &descriptor.import.export_path,
            binding,
        );
    }

    modules
        .into_iter()
        .map(|(module, node)| (module, render_module_source(&node)))
        .collect()
}

/// Runtime binding exported for one contract. Contexts are declarative only: nothing
/// provides their value at runtime, so importing one fails at load instead of
/// silently binding `undefined`.
fn binding_for_descriptor(descriptor: &HostContractDescriptor) -> Option<HostModuleBinding> {
    match &descriptor.abi {
        HostContractAbi::Function { .. } => Some(HostModuleBinding::Function {
            contract_name: descriptor.name.clone(),
        }),
        HostContractAbi::Callback { .. } => Some(HostModuleBinding::Callback {
            event_name: descriptor.name.clone(),
        }),
        HostContractAbi::Context { .. } | HostContractAbi::Unknown => None,
    }
}

fn insert_binding(node: &mut HostModuleNode, path: &[String], binding: HostModuleBinding) {
    let Some((head, tail)) = path.split_first() else {
        return;
    };

    if tail.is_empty() {
        node.children.entry(head.clone()).or_default().binding = Some(binding);
        return;
    }

    insert_binding(
        node.children.entry(head.clone()).or_default(),
        tail,
        binding,
    );
}

fn render_module_source(node: &HostModuleNode) -> String {
    let mut output = String::new();

    for (name, child) in &node.children {
        output.push_str("export const ");
        output.push_str(&identifier(name));
        output.push_str(" = ");
        output.push_str(&render_node(child, 0));
        output.push_str(";\n");
    }

    output
}

fn render_node(node: &HostModuleNode, depth: usize) -> String {
    if let Some(binding) = &node.binding
        && node.children.is_empty()
    {
        return render_binding(binding);
    }

    render_object_node(node, depth)
}

fn render_object_node(node: &HostModuleNode, depth: usize) -> String {
    let mut entries = Vec::new();

    for (name, child) in &node.children {
        entries.push(format!(
            "{}{}: {}",
            indent(depth + 1),
            property_name(name),
            render_node(child, depth + 1)
        ));
    }

    if let Some(binding) = &node.binding {
        entries.push(format!(
            "{}default: {}",
            indent(depth + 1),
            render_binding(binding)
        ));
    }

    if entries.is_empty() {
        return String::from("{}");
    }

    format!("{{\n{}\n{}}}", entries.join(",\n"), indent(depth))
}

fn render_binding(binding: &HostModuleBinding) -> String {
    match binding {
        HostModuleBinding::Function { contract_name } => {
            format!("globalThis.__rustts_native[{contract_name:?}]")
        }
        HostModuleBinding::Callback { event_name } => {
            format!("handler => globalThis.__rustts_on({event_name:?}, handler)")
        }
    }
}

fn identifier(name: &str) -> String {
    if is_identifier(name) {
        name.to_owned()
    } else {
        String::from("sdk")
    }
}

fn property_name(name: &str) -> String {
    if is_identifier(name) && !is_reserved_word(name) {
        name.to_owned()
    } else {
        format!("{name:?}")
    }
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    (first == '_' || first == '$' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric())
        && !is_reserved_word(name)
}

fn is_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "export"
            | "extends"
            | "finally"
            | "for"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
    )
}

fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}
