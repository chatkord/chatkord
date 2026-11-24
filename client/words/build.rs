//! Generates `Phrase`, `Text` and the typed methods on `Words` from the English catalog, so each message is written
//! once, in `catalogs/en.ftl`, and a call that names a message or a value the catalog lacks does not build; and the
//! table of every catalog in `catalogs/`, each named by its language tag, so a language is added by its file alone.
//! Every catalog's `Phrase` is held to a label's bound, so `Words::label` cannot fail.

#[expect(dead_code, reason = "the build reads a catalog; comparing two is the tests'")]
#[path = "src/catalog.rs"]
mod catalog;

use std::error::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use unic_langid::LanguageIdentifier;

const CATALOGS: &str = "catalogs";
/// The catalog every id is generated from and every other one translates.
const ENGLISH: &str = "en";
/// The message each catalog names its own language by.
const OWN_NAME: &str = "language-name";

/// Rust's keywords, and the names `Words` keeps for itself.
const TAKEN: &[&str] = &[
    "as", "async", "await", "box", "break", "const", "continue", "crate", "do", "dyn", "else", "enum", "extern",
    "false", "final", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "macro", "match", "mod", "move", "mut",
    "override", "priv", "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true", "try", "type",
    "typeof", "unsafe", "unsized", "use", "virtual", "where", "while", "yield", "english", "phrase", "say", "format",
    "new", "language",
];

/// A catalog the build carries: its language tag, which is its file's name, and its language's own name.
struct Carried {
    tag: String,
    name: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed={CATALOGS}");
    println!("cargo::rerun-if-changed=src/catalog.rs");
    let mut carried = Vec::new();
    let mut read_all = Vec::new();
    for entry in std::fs::read_dir(CATALOGS)? {
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "ftl") {
            continue;
        }
        let (tag, messages) = read(&path)?;
        let name = own_name(&messages).ok_or_else(|| format!("{}: no `{OWN_NAME}` of plain text", path.display()))?;
        carried.push(Carried { tag: tag.clone(), name });
        read_all.push((tag, messages));
    }
    let english = read_all
        .iter()
        .find_map(|(tag, messages)| (tag == ENGLISH).then_some(messages))
        .ok_or_else(|| format!("{CATALOGS}/{ENGLISH}.ftl: the catalog every id is read from is missing"))?;
    for (tag, messages) in &read_all {
        catalog::labels(english, messages).map_err(|fault| format!("{CATALOGS}/{tag}.ftl: {fault}"))?;
    }
    carried.sort_by(|a, b| (&a.name, &a.tag).cmp(&(&b.name, &b.tag)));
    let code = generate(english)? + &table(&carried)?;
    let out = PathBuf::from(std::env::var("OUT_DIR")?).join("words.rs");
    std::fs::write(out, code)?;
    Ok(())
}

/// The catalog at `path`: its tag, which its file is named by in its canonical form, and its messages.
fn read(path: &Path) -> Result<(String, Vec<catalog::Message>), Box<dyn Error>> {
    let shown = path.display();
    let stem = path.file_stem().and_then(|stem| stem.to_str()).ok_or_else(|| format!("{shown}: not a name"))?;
    let tag: LanguageIdentifier = stem.parse().map_err(|_| format!("{shown}: `{stem}` is not a language tag"))?;
    if tag != stem {
        return Err(format!("{shown}: name it `{tag}.ftl`, its tag's canonical form").into());
    }
    let source = std::fs::read_to_string(path)?;
    let messages = catalog::read(&source).map_err(|fault| format!("{shown}: {fault}"))?;
    Ok((stem.to_string(), messages))
}

/// What a catalog calls its own language: its `language-name`, written without a value or a reference.
fn own_name(messages: &[catalog::Message]) -> Option<String> {
    messages
        .iter()
        .find(|message| message.id == OWN_NAME)
        .filter(|message| message.variables.is_empty() && !message.text.contains(['{', '}']))
        .map(|message| message.text.clone())
}

/// Every catalog carried, in the order of their own names, each read whole into the program; English's named too.
fn table(carried: &[Carried]) -> Result<String, Box<dyn Error>> {
    let catalog = |Carried { tag, name }: &Carried| {
        let source = format!("include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/{CATALOGS}/{tag}.ftl\"))");
        format!("Catalog {{ tag: {tag:?}, name: {name:?}, source: {source} }}")
    };
    let mut code = String::from(
        "/// Every catalog this build carries, in the order of their own names.\nconst CATALOGS: &[Catalog] = &[\n",
    );
    for each in carried {
        let written = if each.tag == ENGLISH { "ENGLISH".to_string() } else { catalog(each) };
        writeln!(code, "    {written},")?;
    }
    code.push_str("];\n\n");
    let english = carried.iter().find(|each| each.tag == ENGLISH).ok_or("no English catalog")?;
    writeln!(code, "/// The catalog every id is generated from.\nconst ENGLISH: Catalog = {};", catalog(english))?;
    Ok(code)
}

fn generate(messages: &[catalog::Message]) -> Result<String, Box<dyn Error>> {
    let (mut labels, mut texts) = (Fixed::default(), Fixed::default());
    let mut methods = String::new();
    let mut structs = String::new();
    for message in messages {
        let docs: String = message.comment.iter().map(|line| format!("    /// {line}\n")).collect();
        let used = message.used();
        if used != catalog::Use::Values {
            let docs = if docs.is_empty() { format!("    /// \"{}\"\n", message.text) } else { docs };
            let variant = camel(&message.id);
            if [&labels, &texts].iter().any(|fixed| fixed.variants.iter().any(|(known, _)| *known == variant)) {
                return Err(format!(
                    "{CATALOGS}/{ENGLISH}.ftl: `{}` reads as `{variant}`, as another id does",
                    message.id
                )
                .into());
            }
            let fixed = if used == catalog::Use::Text { &mut texts } else { &mut labels };
            writeln!(fixed.code, "{docs}    {variant},")?;
            fixed.variants.push((variant, message.id.clone()));
        } else {
            let name = usable(&message.id.replace('-', "_"), &message.id)?;
            let mut sets = String::new();
            let count = message.variables.len();
            // One value is a parameter; two or more are a struct's named fields, so none can be passed for another.
            let parameters = if let [variable] = message.variables.iter().collect::<Vec<_>>()[..] {
                let parameter = usable(variable, &message.id)?;
                writeln!(sets, "        args.set({variable:?}, {parameter});")?;
                format!("{parameter}: impl Into<FluentValue<'a>>")
            } else {
                let values = format!("{}Values", camel(&message.id));
                let mut fields = String::new();
                for variable in &message.variables {
                    let field = usable(variable, &message.id)?;
                    writeln!(fields, "    pub(crate) {field}: FluentValue<'a>,")?;
                    writeln!(sets, "        args.set({variable:?}, values.{field});")?;
                }
                writeln!(
                    structs,
                    "/// The values `{}` names.\npub(crate) struct {values}<'a> {{\n{fields}}}\n",
                    message.id
                )?;
                format!("values: {values}<'a>")
            };
            write!(
                methods,
                "{docs}    pub(crate) fn {name}<'a>(&self, {parameters}) -> String {{\n        let mut args = FluentArgs::with_capacity({count});\n{sets}        self.format({:?}, Some(&args)).into_owned()\n    }}\n\n",
                message.id,
            )?;
        }
    }
    Ok(format!(
        "{}{}{structs}\
         /// The messages that take values, each value named as the catalog names it.\n\
         impl Words {{\n{methods}}}\n",
        labels.written(
            "Phrase",
            "A message of the catalog that takes no value and may name a control: a label, a title, a short sentence. \
             It is plain text, at most a label's length in every catalog.",
        )?,
        texts.written(
            "Text",
            "A message of the catalog that takes no value and is shown as text, never as a control's name: a sentence \
             as long as it needs in each language.",
        )?,
    ))
}

/// The messages of one kind that take no value: each variant's line, and its name with its id.
#[derive(Default)]
struct Fixed {
    code: String,
    variants: Vec<(String, String)>,
}

impl Fixed {
    /// The enum `name`, documented by `doc`, and its ids.
    fn written(&self, name: &str, doc: &str) -> Result<String, std::fmt::Error> {
        let mut ids = String::new();
        for (variant, id) in &self.variants {
            writeln!(ids, "            {name}::{variant} => {id:?},")?;
        }
        Ok(format!(
            "/// {doc}\n\
             #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\n\
             pub enum {name} {{\n{}}}\n\n\
             impl {name} {{\n    /// Its message's id, the same in every catalog.\n    pub const fn id(self) -> &'static str {{\n        match self {{\n{ids}        }}\n    }}\n}}\n\n",
            self.code
        ))
    }
}

fn camel(id: &str) -> String {
    id.split('-')
        .map(|part| {
            let mut letters = part.chars();
            letters.next().map(|first| first.to_uppercase().chain(letters).collect::<String>()).unwrap_or_default()
        })
        .collect()
}

fn usable(name: &str, id: &str) -> Result<String, String> {
    let rust = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && name.starts_with(|c: char| c.is_ascii_lowercase());
    if !rust || TAKEN.contains(&name) {
        return Err(format!(
            "{CATALOGS}/{ENGLISH}.ftl: `{id}` makes `{name}`, which is not a name Rust and `Words` leave free"
        ));
    }
    Ok(name.to_string())
}
