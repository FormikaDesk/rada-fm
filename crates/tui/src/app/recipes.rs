//! Recipes in the interface: the Actions submenu, the bar, `:recipe`, the questions about
//! their settings, and starting one.
//!
//! The interface never runs Lua: it asks the host which recipes fit what is selected, asks
//! the person for the settings with the windows it already has (a menu for a choice, the
//! input window for a text or a number), and hands the rest to a job, which runs the recipe
//! and plans what it asks for. The plan window that follows is the usual one.

use std::sync::Arc;

use rada_core::i18n::{Key, tf};
use rada_core::plugin::{
    ArgKind, ArgValue, ArgValues, Availability, PluginHost, RecipeArg, RecipeContext, RecipeInfo,
};

use super::{App, InputKind, InputView, MenuCmd, MenuItem, MenuTitle, MenuView, Modal};

/// A recipe on its way to running: the settings given so far and the next one to ask.
#[derive(Clone, Debug, PartialEq)]
pub struct RecipeAsk {
    pub name: String,
    pub title: String,
    pub ctx: RecipeContext,
    pub args: Vec<RecipeArg>,
    pub values: ArgValues,
}

impl RecipeAsk {
    /// The first setting not given yet.
    fn next(&self) -> Option<&RecipeArg> {
        self.args
            .iter()
            .find(|a| !self.values.contains_key(&a.name))
    }

    fn with(&self, name: &str, value: ArgValue) -> RecipeAsk {
        let mut next = self.clone();
        next.values.insert(name.to_string(), value);
        next
    }
}

impl App {
    /// Give the interface the recipes of `host` (the tests give their own).
    pub fn use_plugins(&mut self, host: Option<Arc<dyn PluginHost>>) {
        self.svc.plugins = host;
    }

    /// What a recipe would be run on now: what is selected, in a folder. None in an archive,
    /// while choosing a file, or with recipes off.
    pub(super) fn recipe_context(&self) -> Option<RecipeContext> {
        self.svc.plugins.as_ref()?;
        if self.tab.archive.is_some() || self.session.chooser.is_some() {
            return None;
        }
        let selection = self.tab.targets();
        let cwd = match self.tab.virtual_view() {
            // A list that is not a folder: the folder of the first item.
            Some(_) => selection.first()?.parent()?.to_path_buf(),
            None => self.tab.cwd.clone(),
        };
        let marked = if self.tab.sel.marked.is_empty() {
            Vec::new()
        } else {
            selection.clone()
        };
        Some(RecipeContext {
            selection,
            marked,
            cwd,
            os: rada_core::ops::programs::Os::current(),
        })
    }

    /// The recipes for what is selected, those that cannot run now among them.
    pub fn recipe_list(&self) -> Vec<RecipeInfo> {
        match (&self.svc.plugins, self.recipe_context()) {
            (Some(host), Some(ctx)) if !ctx.selection.is_empty() => host.recipes(&ctx),
            _ => Vec::new(),
        }
    }

    /// The entries of the Actions submenu: a recipe that cannot run is greyed, with why.
    pub(super) fn recipe_menu_items(&self) -> Vec<MenuItem> {
        let Some(ctx) = self.recipe_context() else {
            return Vec::new();
        };
        self.recipe_list()
            .into_iter()
            .map(|info| {
                let why = match &info.state {
                    Availability::Available => None,
                    Availability::Disabled(why) => Some(why.clone()),
                };
                let ask = RecipeAsk {
                    name: info.name,
                    title: info.title.clone(),
                    ctx: ctx.clone(),
                    args: info.args,
                    values: ArgValues::new(),
                };
                MenuItem::recipe(Box::new(ask), &info.title, why)
            })
            .collect()
    }

    /// Start the recipe `name` with the settings `given`; the others are asked for. `Err`
    /// says why it cannot start.
    pub fn start_recipe(&mut self, name: &str, given: ArgValues) -> Result<(), String> {
        let ctx = self
            .recipe_context()
            .ok_or_else(|| Key::WhyNotHere.tr().to_string())?;
        let info = self
            .recipe_list()
            .into_iter()
            .find(|r| r.name == name)
            .ok_or_else(|| tf(Key::RecipeNoneHere, &[&rada_core::display::text(name)]))?;
        if let Availability::Disabled(why) = info.state {
            return Err(why);
        }
        for (key, value) in &given {
            let Some(arg) = info.args.iter().find(|a| &a.name == key) else {
                return Err(format!("{key}: {}", Key::RecipeArgUnknown.tr()));
            };
            if !arg.accepts(value) {
                return Err(format!("{key}: {}", Key::RecipeArgWrong.tr()));
            }
        }
        self.continue_recipe(RecipeAsk {
            name: info.name,
            title: info.title,
            ctx,
            args: info.args,
            values: given,
        });
        Ok(())
    }

    /// Ask the next setting, or run the recipe when they are all there.
    pub(super) fn continue_recipe(&mut self, ask: RecipeAsk) {
        let Some(arg) = ask.next().cloned() else {
            return self.launch_recipe(ask);
        };
        match &arg.kind {
            ArgKind::Choice(words) => {
                let values: Vec<(String, ArgValue)> = words
                    .iter()
                    .map(|w| (w.clone(), ArgValue::Text(w.clone())))
                    .collect();
                self.ask_recipe_choice(ask, &arg, values);
            }
            ArgKind::Flag => {
                let values = vec![
                    (Key::BtnYes.tr().to_string(), ArgValue::Flag(true)),
                    (Key::BtnNo.tr().to_string(), ArgValue::Flag(false)),
                ];
                self.ask_recipe_choice(ask, &arg, values);
            }
            ArgKind::Text | ArgKind::Int { .. } => {
                let text = match &arg.default {
                    Some(ArgValue::Text(t)) => t.clone(),
                    Some(ArgValue::Int(n)) => n.to_string(),
                    _ => String::new(),
                };
                let kind = InputKind::RecipeArg {
                    ask: Box::new(ask),
                    arg: arg.clone(),
                };
                self.windows.modal = Some(Modal::Input(InputView::new(kind, text)));
            }
        }
    }

    /// A setting with a few values: a menu of them, the default ticked and under the cursor.
    fn ask_recipe_choice(
        &mut self,
        ask: RecipeAsk,
        arg: &RecipeArg,
        values: Vec<(String, ArgValue)>,
    ) {
        let items: Vec<MenuItem> = values
            .iter()
            .map(|(label, value)| {
                let on = arg.default.as_ref() == Some(value);
                MenuItem::choice(
                    MenuCmd::Recipe(Box::new(ask.with(&arg.name, value.clone()))),
                    label,
                    on,
                    false,
                )
            })
            .collect();
        let selected = items
            .iter()
            .position(|i| i.checked == Some(true))
            .unwrap_or(0);
        let title = MenuTitle {
            icon: "",
            icon_color: self.settings.th.accent,
            name: format!("{} · {}", ask.title, arg.label),
            kind: String::new(),
        };
        let mut menu = MenuView::new(self.menu_anchor(), items).titled(Some(title));
        menu.selected = selected;
        self.windows.modal = Some(Modal::Menu(menu));
    }

    /// Where a menu that no click opened goes: by the item under the cursor.
    fn menu_anchor(&self) -> (u16, u16) {
        self.frame
            .hits
            .find(&crate::hits::Target::Row(self.tab.sel.cursor))
            .map(|r| (r.x.saturating_add(6), r.y.saturating_add(1)))
            .unwrap_or((8, 6))
    }

    /// Enter in the window of a text or a number.
    pub(super) fn submit_recipe_text(&mut self, ask: RecipeAsk, arg: RecipeArg, iv: InputView) {
        let value = match arg.kind {
            ArgKind::Int { .. } => match iv.text.trim().parse::<i64>() {
                Ok(n) => ArgValue::Int(n),
                Err(_) => return self.reject_input(iv, Key::RecipeArgWrong.tr().to_string()),
            },
            _ => ArgValue::Text(iv.text.clone()),
        };
        if !arg.accepts(&value) {
            return self.reject_input(iv, Key::RecipeArgWrong.tr().to_string());
        }
        self.continue_recipe(ask.with(&arg.name, value));
    }

    /// Run the recipe and plan what it asks for, on a job: the window says it is being
    /// prepared, Esc stops it.
    fn launch_recipe(&mut self, ask: RecipeAsk) {
        if self.refuse_if_busy() {
            return;
        }
        let Some(host) = self.svc.plugins.clone() else {
            return;
        };
        let what = tf(Key::PreparingRecipe, &[&ask.title]);
        let handle = self
            .svc
            .jobs
            .plan_recipe(host, ask.name, ask.ctx, ask.values);
        self.begin_plan(&what, handle);
    }

    /// The recipes the bar may offer: those that can run on what is selected.
    pub(super) fn bar_recipes(&self) -> Vec<(String, String)> {
        self.recipe_list()
            .into_iter()
            .filter(|r| r.state == Availability::Available)
            .map(|r| (r.name, r.title))
            .collect()
    }

    /// `:recipe name key=value…`, or `:name key=value…`: start it with those settings.
    pub fn recipe_by_words(&mut self, line: &str) -> Result<(), String> {
        let mut words = line.split_whitespace();
        let Some(name) = words.next() else {
            return Err(Key::RecipeWhich.tr().to_string());
        };
        let mut given = ArgValues::new();
        let args = self
            .recipe_list()
            .into_iter()
            .find(|r| r.name == name)
            .map(|r| r.args)
            .unwrap_or_default();
        for word in words {
            let Some((key, text)) = word.split_once('=') else {
                return Err(tf(Key::RecipeSetting, &[&rada_core::display::text(word)]));
            };
            let kind = args.iter().find(|a| a.name == key).map(|a| &a.kind);
            given.insert(key.to_string(), typed_value(kind, text));
        }
        self.start_recipe(name, given)
    }

    /// `:recipe name key=value…` from the registry.
    pub fn recipe_command(&mut self, args: &crate::registry::Args) -> Result<(), String> {
        let line = args.text("recipe").unwrap_or_default().to_string();
        self.recipe_by_words(&line)
    }

    /// The command line: `:recipe …` through the registry, or a line that starts with the
    /// name of a recipe. `None` when it is neither.
    pub(super) fn recipe_line(&mut self, line: &str) -> Option<Result<(), String>> {
        let first = line.split_whitespace().next()?;
        let known = self.recipe_list().iter().any(|r| r.name == first);
        known.then(|| self.recipe_by_words(line))
    }
}

/// A value typed on the command line, as the setting takes it.
fn typed_value(kind: Option<&ArgKind>, text: &str) -> ArgValue {
    match kind {
        Some(ArgKind::Int { .. }) => text
            .parse()
            .map(ArgValue::Int)
            .unwrap_or_else(|_| ArgValue::Text(text.to_string())),
        Some(ArgKind::Flag) => match text {
            "true" | "yes" | "on" | "si" => ArgValue::Flag(true),
            "false" | "no" | "off" => ArgValue::Flag(false),
            _ => ArgValue::Text(text.to_string()),
        },
        _ => ArgValue::Text(text.to_string()),
    }
}
