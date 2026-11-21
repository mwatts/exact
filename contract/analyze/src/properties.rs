//! Root property reads closed over derive and function bodies (LLP 1151 D1).

use super::AnalyzeError;
use contract_syntax::inline::calls::shown;
use contract_syntax::{
    Component, Expanded, Expr, FnDecl, PropertyDecl, PropertyForm, PropertyKind,
};
use std::collections::BTreeSet;

/// Resolve the action each `after` and `never refuses` line names to the
/// expanded root's actions ([`PropertyDecl::actions`]). A bare name is a
/// root action's, as the runner prints it; one only a child declares is
/// refused with its qualified form, and one nothing declares stays as written
/// and is not exercised. `Component.action` names that action of every
/// instance of `Component`, whose expanded names are generated (`action#<hash>`),
/// and is refused when it names none; `.` cannot appear in an authored
/// identifier, so the two never collide.
pub fn resolve_actions(expanded: &Expanded, properties: &mut [PropertyDecl]) -> Vec<AnalyzeError> {
    let actions = &expanded.root.actions;
    let component = |i: usize| {
        let instance = *expanded.action_instances.get(i)?;
        Some(
            expanded
                .instances
                .get(instance as usize)?
                .component
                .as_str(),
        )
    };
    let mut errors = Vec::new();
    for p in properties.iter_mut() {
        let written = match &p.kind {
            PropertyKind::After(action) => action.clone(),
            PropertyKind::NeverRefuses => p.name.clone(),
            PropertyKind::Always | PropertyKind::Clock => continue,
        };
        let message = match written.split_once('.') {
            None if actions.iter().any(|a| a.name == written) => {
                p.actions = vec![written];
                continue;
            }
            None => {
                let owners: BTreeSet<_> = (0..actions.len())
                    .filter(|&i| shown(&actions[i].name) == written)
                    .filter_map(component)
                    .map(|c| format!("`{c}.{written}`"))
                    .collect();
                if owners.is_empty() {
                    p.actions = vec![written];
                    continue;
                }
                let owners: Vec<_> = owners.into_iter().collect();
                format!(
                    "`{written}` is a child's action; name it by its component: {}",
                    owners.join(" or ")
                )
            }
            Some((name, action)) => {
                let found: Vec<_> = (0..actions.len())
                    .filter(|&i| component(i) == Some(name) && shown(&actions[i].name) == action)
                    .map(|i| actions[i].name.clone())
                    .collect();
                if !found.is_empty() {
                    p.actions = found;
                    continue;
                }
                if expanded.instances.iter().any(|i| i.component == name) {
                    let own: BTreeSet<_> = (0..actions.len())
                        .filter(|&i| component(i) == Some(name))
                        .map(|i| shown(&actions[i].name))
                        .collect();
                    let own: Vec<_> = own.into_iter().collect();
                    format!(
                        "`{name}` has no action `{action}`; its actions: {}",
                        if own.is_empty() {
                            "none".into()
                        } else {
                            own.join(", ")
                        }
                    )
                } else {
                    format!("the app uses no component `{name}`")
                }
            }
        };
        errors.push(AnalyzeError {
            id: "analyze-property-action",
            message,
            span: p.span,
            related: Vec::new(),
        });
    }
    errors
}

/// Refuse unknown names and derives whose values depend on the oracle, clock or geometry.
/// An `always` line may read a derive over resources: it checks each printed
/// configuration, where both sides print the oracle's answer, so it never
/// pairs a value with a commit as `after` does. `root` is the expanded
/// component that the plan compiler uses.
pub fn check_properties(
    root: &Component,
    fns: &[FnDecl],
    properties: &[PropertyDecl],
) -> Vec<AnalyzeError> {
    let mut errors = Vec::new();
    for property in properties {
        for line in &property.lines {
            let names: Vec<&str> = match &line.form {
                PropertyForm::Hold { name, .. } => vec![name],
                PropertyForm::OnlyWrites(names) => names.iter().map(String::as_str).collect(),
                PropertyForm::Command { .. } => Vec::new(),
            };
            for name in names {
                let slot = root.states.iter().any(|s| s.name == name)
                    || root.mutations.iter().any(|m| m.name == name);
                if slot {
                    continue;
                }
                let mut cone = Cone {
                    root,
                    fns,
                    seen: BTreeSet::new(),
                    resources: property.kind == PropertyKind::Always,
                };
                let reason = if let Some(d) = root.derives.iter().find(|d| d.name == name) {
                    if matches!(line.form, PropertyForm::OnlyWrites(_)) {
                        Some(format!(
                            "`{name}` is a derive; `only writes` names root slots"
                        ))
                    } else {
                        cone.expr(&d.expr, &BTreeSet::new()).map(|chain| format!("`{name}` -> {chain}: its value would be the oracle's or would change on a step that commits nothing"))
                    }
                } else if root.resources.iter().any(|r| r.name == name) {
                    Some(format!("`{name}` is a resource: its value would be the oracle's or would change on a step that commits nothing"))
                } else {
                    Some(format!("`{name}` is not a root state or derive"))
                };
                if let Some(message) = reason {
                    errors.push(AnalyzeError {
                        id: "analyze-property-read",
                        message,
                        span: line.span,
                        related: Vec::new(),
                    });
                }
            }
        }
    }
    errors
}

struct Cone<'a> {
    root: &'a Component,
    fns: &'a [FnDecl],
    seen: BTreeSet<(&'static str, String)>,
    /// Whether a resource's value may be read.
    resources: bool,
}
impl Cone<'_> {
    fn base<'a>(e: &'a Expr, bound: &BTreeSet<String>) -> Option<&'a str> {
        match e {
            Expr::Ident(name, _) if !bound.contains(name) => Some(name),
            Expr::Member(base, _, _) => Self::base(base, bound),
            _ => None,
        }
    }

    fn expr(&mut self, e: &Expr, bound: &BTreeSet<String>) -> Option<String> {
        match e {
            Expr::Ident(name, _) if !bound.contains(name) => {
                if self.root.resources.iter().any(|r| r.name == *name) {
                    return (!self.resources).then(|| format!("resource `{name}`"));
                }
                if self.seen.insert(("derive", name.clone())) {
                    if let Some(d) = self.root.derives.iter().find(|d| d.name == *name) {
                        return self
                            .expr(&d.expr, &BTreeSet::new())
                            .map(|s| format!("`{name}` -> {s}"));
                    }
                }
                None
            }
            Expr::Call(name, args, _) => {
                if contract_syntax::scope::STATUS_CALLS.contains(&name.as_str()) {
                    if let Some(target) = args.first().and_then(|arg| Self::base(arg, bound)) {
                        let kind = if self.root.mutations.iter().any(|m| m.name == target) {
                            "mutation"
                        } else if self.root.resources.iter().any(|r| r.name == target) {
                            "resource"
                        } else {
                            "target"
                        };
                        return Some(format!("`{name}(…)` of {kind} `{target}`"));
                    }
                } else if let Some(f) = self.fns.iter().find(|f| f.name == *name) {
                    if self.seen.insert(("fn", name.clone())) {
                        let locals = f.params.iter().map(|p| p.name.clone()).collect();
                        if let Some(s) = self.expr(&f.body, &locals) {
                            return Some(format!("fn `{name}` -> {s}"));
                        }
                    }
                } else if matches!(name.as_str(), "performanceNow" | "frame" | "measure") {
                    // The roster's date formatters read their arguments; exactTime is a resource.
                    return Some(format!("`{name}(…)`"));
                }
                args.iter().find_map(|a| self.expr(a, bound))
            }
            _ => e.scoped_operands().find_map(|(x, binds)| {
                if binds.is_empty() {
                    return self.expr(x, bound);
                }
                let mut locals = bound.clone();
                locals.extend(binds.iter().map(String::from));
                self.expr(x, &locals)
            }),
        }
    }
}
