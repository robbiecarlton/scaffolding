use crate::error::{ScaffoldError, ScaffoldResult};
use crate::file_tree::{dir_content, file_content, load_directory_into_memory, FileTree};
use crate::scaffold::app::cargo::exec_metadata;
use crate::scaffold::app::AppFileTree;
use crate::scaffold::dna::{scaffold_dna, DnaFileTree};
use crate::scaffold::entry_type::crud::{parse_crud, Crud};
use crate::scaffold::entry_type::definitions::{
    parse_entry_type_reference, parse_referenceable, EntryTypeReference, FieldType, Referenceable,
};
use crate::scaffold::entry_type::{parse_depends_on_itself, scaffold_entry_type, DependsOnItself};
use crate::scaffold::index::{scaffold_index, IndexType};
use crate::scaffold::link_type::scaffold_link_type;
use crate::scaffold::web_app::scaffold_web_app;
use crate::scaffold::web_app::uis::{choose_ui_framework, template_for_ui_framework, UiFramework};
use crate::scaffold::zome::utils::select_integrity_zomes;
use crate::scaffold::zome::{
    integrity_zome_name, scaffold_coordinator_zome, scaffold_integrity_zome, ZomeFileTree,
};
use crate::templates::get::get_template;
use crate::templates::{choose_or_get_template, choose_or_get_template_file_tree, templates_path};
use crate::utils::{
    check_no_whitespace, check_snake_case, input_no_whitespace, input_snake_case, input_yes_or_no,
};

use build_fs_tree::{dir, Build, MergeableFileSystemTree};
use dialoguer::Input;
use dialoguer::{theme::ColorfulTheme, Select};
use std::process::Stdio;
use std::str::FromStr;
use std::{ffi::OsString, path::PathBuf, process::Command};
use structopt::StructOpt;

/// The list of subcommands for `hc scaffold`
#[derive(Debug, StructOpt)]
#[structopt(setting = structopt::clap::AppSettings::InferSubcommands)]
pub enum HcScaffold {
    /// Scaffold a new, empty web app
    WebApp {
        /// Name of the app to scaffold
        name: Option<String>,

        /// [OPTIONAL] Description of the app to scaffold
        description: Option<String>,

        #[structopt(long)]
        /// Whether to setup the holonix development environment for this web-app
        setup_nix: Option<bool>,

        #[structopt(short = "u", long)]
        /// The git repository URL from which to download the template, incompatible with --templates-path
        templates_url: Option<String>,

        #[structopt(short = "p", long)]
        /// The local folder path in which to look for the given template, incompatible with --templates-url
        templates_path: Option<PathBuf>,

        #[structopt(short, long)]
        /// The template to scaffold the web-app from
        /// If "--templates-url" is given, the template must be located at the ".templates/<TEMPLATE NAME>" folder of the repository
        /// If not, the template must be an option from the built-in templates: "vanilla", "vue", "lit", "svelte"
        template: Option<String>,
    },
    /// Set up the template used in this project
    Template(HcScaffoldTemplate),
    /// Scaffold a DNA into an existing app
    Dna {
        #[structopt(long)]
        /// Name of the app in which you want to scaffold the DNA
        app: Option<String>,

        /// Name of the DNA being scaffolded
        name: Option<String>,

        #[structopt(short, long)]
        /// The template to scaffold the dna from
        /// The template must be located at the ".templates/<TEMPLATE NAME>" folder of the repository
        template: Option<String>,
    },
    /// Scaffold one or multiple zomes into an existing DNA
    Zome {
        #[structopt(long)]
        /// Name of the dna in which you want to scaffold the zome
        dna: Option<String>,

        /// Name of the zome being scaffolded
        name: Option<String>,

        #[structopt(long)]
        /// Scaffold an integrity zome at the given path
        integrity: Option<PathBuf>,

        #[structopt(long)]
        /// Scaffold a coordinator zome at the given path
        coordinator: Option<PathBuf>,

        #[structopt(short, long)]
        /// The template to scaffold the dna from
        /// The template must be located at the ".templates/<TEMPLATE NAME>" folder of the repository
        template: Option<String>,
    },
    /// Scaffold an entry type and CRUD functions into an existing zome
    EntryType {
        #[structopt(long)]
        /// Name of the dna in which you want to scaffold the zome
        dna: Option<String>,

        #[structopt(long)]
        /// Name of the integrity zome in which you want to scaffold the entry definition
        zome: Option<String>,

        /// Name of the entry type being scaffolded
        name: Option<String>,

        #[structopt(long)]
        /// Whether this entry should be "fixed" or not
        /// Fixed entries cannot be updated or deleted, and they are referenced to by with its "EntryHash"
        fixed: Option<bool>,

        #[structopt(long, parse(try_from_str = parse_crud))]
        /// The Create, "Read", "Update", and "Delete" zome call functions that should be scaffolded for this entry type
        /// If "--reference-entry-hash" is "true", only "Create" and "Read" will be scaffolded
        crud: Option<Crud>,

        #[structopt(long)]
        /// Whether to create a link from the original entry to each update action
        /// Only applies if update is selected in the "crud" argument
        link_from_original_to_each_update: Option<bool>,

        #[structopt(long, value_delimiter = ",", parse(try_from_str = parse_referenceable))]
        /// The entry types (or agent roles) that the new entry type depends on
        depends_on: Option<Vec<Referenceable>>,

        #[structopt(long, parse(try_from_str = parse_depends_on_itself))]
        /// The fields that the entry type struct should contain
        depends_on_itself: Option<DependsOnItself>,

        #[structopt(long, value_delimiter = ",", parse(try_from_str = parse_fields))]
        /// The fields that the entry type struct should contain
        fields: Option<Vec<(String, FieldType)>>,

        #[structopt(short, long)]
        /// The template to scaffold the dna from
        /// The template must be located at the ".templates/<TEMPLATE NAME>" folder of the repository
        template: Option<String>,
    },
    /// Scaffold a link type and its appropriate zome functions into an existing zome
    LinkType {
        #[structopt(long)]
        /// Name of the dna in which you want to scaffold the zome
        dna: Option<String>,

        #[structopt(long)]
        /// Name of the integrity zome in which you want to scaffold the link type
        zome: Option<String>,

        #[structopt(parse(try_from_str = parse_referenceable))]
        /// Entry type (or agent role) used as the base for the links
        from_referenceable: Option<Referenceable>,

        #[structopt(parse(try_from_str = parse_referenceable))]
        /// Entry type (or agent role) used as the target for the links
        to_referenceable: Option<Referenceable>,

        #[structopt(long)]
        /// Whether to create the inverse link, from the "--to-referenceable" entry type to the "--fromreferenceable" entry type
        bidireccional: Option<bool>,

        #[structopt(short, long)]
        /// The template to scaffold the dna from
        /// The template must be located at the ".templates/<TEMPLATE NAME>" folder of the repository
        template: Option<String>,
    },
    /// Scaffold an indexing link-type and appropriate zome functions to index entries into an existing zome
    Index {
        #[structopt(long)]
        /// Name of the dna in which you want to scaffold the zome
        dna: Option<String>,

        #[structopt(long)]
        /// Name of the integrity zome in which you want to scaffold the link type
        zome: Option<String>,

        /// Index type: "global" or "by-author"
        index_type: Option<IndexType>,

        /// Index name, just to differentiate it from other indexes
        index_name: Option<String>,

        #[structopt(parse(try_from_str = parse_entry_type_reference))]
        /// Entry type that is going to be indexed by this index
        entry_type: Option<EntryTypeReference>,

        #[structopt(short, long)]
        /// The template to scaffold the dna from
        /// The template must be located at the ".templates/<TEMPLATE NAME>" folder of the repository
        template: Option<String>,
    },
}

pub fn parse_fields(_fields_str: &str) -> Result<(String, FieldType), String> {
    Err(String::from("TODO!"))
}

impl HcScaffold {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            HcScaffold::WebApp {
                name,
                description,
                setup_nix,
                template,
                templates_url,
                templates_path,
            } => {
                let prompt = String::from("App name (no whitespaces):");
                let name: String = match name {
                    Some(n) => check_no_whitespace(n, "app name")?,
                    None => input_no_whitespace(&prompt)?,
                };

                let current_dir = std::env::current_dir()?;
                let app_folder = current_dir.join(&name);
                if app_folder.as_path().exists() {
                    return Err(ScaffoldError::FolderAlreadyExists(app_folder.clone()))?;
                }

                let (template_name, template_file_tree, scaffold_template) =
                    match (templates_url, templates_path) {
                        (Some(_), Some(_)) => Err(ScaffoldError::InvalidArguments(String::from(
                            "cannot use --templates-path and --templates-url together",
                        )))?,
                        (Some(u), None) => {
                            let (name, file_tree) = get_template(&u, &template)?;
                            (name, file_tree, true)
                        }
                        (None, Some(p)) => {
                            let templates_dir = current_dir.join(p);
                            let templates_file_tree = load_directory_into_memory(&templates_dir)?;
                            let name = choose_or_get_template(
                                &dir! {".templates"=>templates_file_tree},
                                &template,
                            )?;
                            let template_file_tree =
                                load_directory_into_memory(&templates_dir.join(&name))?;
                            (name, template_file_tree, true)
                        }
                        (None, None) => {
                            let ui_framework = match template {
                                Some(t) => UiFramework::from_str(t.as_str())?,
                                None => choose_ui_framework()?,
                            };

                            (
                                format!("{:?}", ui_framework),
                                template_for_ui_framework(&ui_framework)?,
                                false,
                            )
                        }
                    };

                if file_content(&template_file_tree, &PathBuf::from("web-app/README.md.hbs"))
                    .is_err()
                {
                    return Err(ScaffoldError::MalformedTemplate(
                        "Template does not contain a README.md.hbs file in its \"web-app\" directory"
                            .to_string(),
                    ))?;
                }

                let setup_nix = match setup_nix {
                    Some(s) => s,
                    None => {
                        let holonix_prompt = String::from("Do you want to set up the holonix development environment for this project?");
                        input_yes_or_no(&holonix_prompt, Some(true))?
                    }
                };

                let app_file_tree = scaffold_web_app(
                    name.clone(),
                    description,
                    !setup_nix,
                    template_file_tree,
                    template_name,
                    scaffold_template,
                )?;

                let file_tree = MergeableFileSystemTree::<OsString, String>::from(app_file_tree);

                file_tree.build(&".".into())?;

                let mut maybe_nix = "";

                if setup_nix {
                    if cfg!(target_os = "windows") {
                        return Err(anyhow::anyhow!("Windows doesn't support nix"));
                    } else {
                        let output = Command::new("nix-shell")
                        .stdout(Stdio::inherit())
                        .current_dir(std::env::current_dir()?.join(&name))
                        .args(["-I", "nixpkgs=https://github.com/NixOS/nixpkgs/archive/nixos-21.11.tar.gz", "-p", "niv", "--run", "niv init && niv drop nixpkgs && niv drop niv && niv add -b main holochain/holonix"])
                        .output()?;

                        if !output.status.success() {
                            return Err(ScaffoldError::NixSetupError)?;
                        }

                        maybe_nix = "\n  nix-shell";
                    };
                }

                println!(
                    r#"
Web hApp "{}" scaffolded!

Set up your development environment as described in {}/README.md.

Then, add new DNAs to your app with:

  hc-scaffold dna
"#,
                    name, name
                );
            }
            HcScaffold::Template(template) => template.run()?,
            HcScaffold::Dna {
                app,
                name,
                template,
            } => {
                let prompt = String::from("DNA name (snake_case):");
                let name: String = match name {
                    Some(n) => check_snake_case(n, "dna name")?,
                    None => input_snake_case(&prompt)?,
                };

                let current_dir = std::env::current_dir()?;

                let file_tree = load_directory_into_memory(&current_dir)?;
                let template_file_tree = choose_or_get_template_file_tree(&file_tree, &template)?;

                let app_file_tree = AppFileTree::get_or_choose(file_tree, &app)?;

                let file_tree = scaffold_dna(app_file_tree, &template_file_tree, &name)?;

                let file_tree =
                    MergeableFileSystemTree::<OsString, String>::from(file_tree.file_tree());

                file_tree.build(&".".into())?;

                println!(
                    r#"
DNA "{}" scaffolded!

Add new zomes to your DNA with:

  hc-scaffold zome
"#,
                    name
                );
            }
            HcScaffold::Zome {
                dna,
                name,
                integrity,
                coordinator,
                template,
            } => {
                let current_dir = std::env::current_dir()?;
                let file_tree = load_directory_into_memory(&current_dir)?;
                let template_file_tree = choose_or_get_template_file_tree(&file_tree, &template)?;

                if let Some(n) = name.clone() {
                    check_snake_case(n, "zome name")?;
                }

                let (scaffold_integrity, scaffold_coordinator) =
                    match (integrity.clone(), coordinator.clone()) {
                        (None, None) => {
                            let option = Select::with_theme(&ColorfulTheme::default())
                                .with_prompt("What do you want to scaffold?")
                                .default(0)
                                .items(&[
                                    "Integrity/coordinator zome-pair (recommended)",
                                    "Only an integrity zome",
                                    "Only a coordinator zome",
                                ])
                                .interact()?;

                            match option {
                                0 => (true, true),
                                1 => (true, false),
                                _ => (false, true),
                            }
                        }
                        _ => (integrity.is_some(), coordinator.is_some()),
                    };

                let name_prompt = match (scaffold_integrity, scaffold_coordinator) {
                    (true, true) => String::from("Enter coordinator zome name (snake_case):\n (The integrity zome will automatically be named '{name of coordinator zome}_integrity')\n"),
                    _ => String::from("Enter zome name (snake_case):"),
                };

                let name: String = match name {
                    Some(n) => n,
                    None => input_snake_case(&name_prompt)?,
                };

                let mut dna_file_tree = DnaFileTree::get_or_choose(file_tree, &dna)?;

                if scaffold_integrity {
                    let integrity_zome_name = match scaffold_coordinator {
                        true => integrity_zome_name(&name),
                        false => name.clone(),
                    };
                    let zome_file_tree = scaffold_integrity_zome(
                        dna_file_tree,
                        &template_file_tree,
                        &integrity_zome_name,
                        &integrity,
                    )?;
                    dna_file_tree = zome_file_tree.dna_file_tree;
                }

                if scaffold_coordinator {
                    let dependencies = match scaffold_integrity {
                        true => Some(vec![integrity_zome_name(&name)]),
                        false => {
                            Some(select_integrity_zomes(&dna_file_tree.dna_manifest, Some(&String::from(
                        "Select integrity zome(s) this coordinator zome depends on (SPACE to select/unselect, ENTER to continue):"
                        )))?
                    )
                        }
                    };
                    let zome_file_tree = scaffold_coordinator_zome(
                        dna_file_tree,
                        &template_file_tree,
                        &name,
                        &dependencies,
                        &coordinator,
                    )?;
                    dna_file_tree = zome_file_tree.dna_file_tree;
                }

                // TODO: implement scaffold_zome_template

                let file_tree =
                    MergeableFileSystemTree::<OsString, String>::from(dna_file_tree.file_tree());

                let f = file_tree.clone();

                file_tree.build(&".".into())?;

                // Execute cargo metadata to set up the cargo workspace in case this zome is the first crate
                exec_metadata(&f)?;

                let headline = match (scaffold_integrity, scaffold_coordinator) {
                    (true, false) => format!(r#"Integrity zome "{}" scaffolded!"#, name),
                    (false, true) => format!(r#"Coordinator zome "{}" scaffolded!"#, name),
                    (_, _) => format!(
                        r#"Integrity zome "{}" and coordinator zome "{}" scaffolded!"#,
                        integrity_zome_name(&name),
                        name
                    ),
                };

                println!(
                    r#"
{}

Add new entry definitions to your zome with:

  hc-scaffold entry-type
"#,
                    headline
                );
            }
            HcScaffold::EntryType {
                dna,
                zome,
                name,
                crud,
                fixed: reference_entry_hash,
                link_from_original_to_each_update,
                depends_on,
                depends_on_itself,
                fields,
                template,
            } => {
                let current_dir = std::env::current_dir()?;
                let file_tree = load_directory_into_memory(&current_dir)?;
                let template_file_tree = choose_or_get_template_file_tree(&file_tree, &template)?;

                let name: String = match name {
                    Some(n) => check_snake_case(n, "entry type name")?,
                    None => input_snake_case(&String::from("Entry type name (snake_case):"))?,
                };

                let dna_file_tree = DnaFileTree::get_or_choose(file_tree, &dna)?;

                let zome_file_tree = ZomeFileTree::get_or_choose_integrity(dna_file_tree, &zome)?;

                let app_file_tree = scaffold_entry_type(
                    zome_file_tree,
                    &template_file_tree,
                    &name,
                    &crud,
                    &reference_entry_hash,
                    &link_from_original_to_each_update,
                    &depends_on,
                    &depends_on_itself,
                    &fields,
                )?;

                let file_tree = MergeableFileSystemTree::<OsString, String>::from(app_file_tree);

                file_tree.build(&".".into())?;

                println!(
                    r#"
Entry type "{}" scaffolded!

Add new indexes for that entry type with:

  hc-scaffold index
"#,
                    name
                );
            }
            HcScaffold::LinkType {
                dna,
                zome,
                from_referenceable,
                to_referenceable,
                bidireccional,
                template,
            } => {
                let current_dir = std::env::current_dir()?;
                let file_tree = load_directory_into_memory(&current_dir)?;
                let template_file_tree = choose_or_get_template_file_tree(&file_tree, &template)?;

                let dna_file_tree = DnaFileTree::get_or_choose(file_tree, &dna)?;

                let zome_file_tree = ZomeFileTree::get_or_choose_integrity(dna_file_tree, &zome)?;

                let (app_file_tree, link_type_name) = scaffold_link_type(
                    zome_file_tree,
                    &template_file_tree,
                    &from_referenceable,
                    &to_referenceable,
                    &bidireccional,
                )?;

                let file_tree = MergeableFileSystemTree::<OsString, String>::from(app_file_tree);

                file_tree.build(&".".into())?;

                println!(
                    r#"
Link type "{}" scaffolded!
"#,
                    link_type_name
                );
            }
            HcScaffold::Index {
                dna,
                zome,
                index_name,
                index_type,
                entry_type,
                template,
            } => {
                let current_dir = std::env::current_dir()?;
                let file_tree = load_directory_into_memory(&current_dir)?;
                let template_file_tree = choose_or_get_template_file_tree(&file_tree, &template)?;

                let dna_file_tree = DnaFileTree::get_or_choose(file_tree, &dna)?;

                let zome_file_tree = ZomeFileTree::get_or_choose_integrity(dna_file_tree, &zome)?;

                let prompt = String::from("Index name (snake_case, eg. \"all_posts\"):");
                let name: String = match index_name {
                    Some(n) => check_snake_case(n, "index name")?,
                    None => input_snake_case(&prompt)?,
                };

                let app_file_tree = scaffold_index(
                    zome_file_tree,
                    &template_file_tree,
                    &name,
                    &index_type,
                    &entry_type,
                )?;

                let file_tree = MergeableFileSystemTree::<OsString, String>::from(app_file_tree);

                file_tree.build(&".".into())?;

                println!(
                    r#"
Index "{}" scaffolded!
"#,
                    name
                );
            }
        }

        Ok(())
    }
}

#[derive(Debug, StructOpt)]
#[structopt(setting = structopt::clap::AppSettings::InferSubcommands)]
pub enum HcScaffoldTemplate {
    Get {
        /// The git repository URL from which to download the template
        template_url: String,

        #[structopt(long)]
        /// The template to get from the given repository (located at ".templates/<FROM TEMPLATE>")
        from_template: Option<String>,

        #[structopt(long)]
        /// The folder to download the template to, will end up at ".templates/<TO TEMPLATE>"
        to_template: Option<String>,
    },
    Init {
        /// The UI framework to use as the template for this web-app
        template: Option<UiFramework>,

        #[structopt(long)]
        /// The folder to download the template to, will end up at ".templates/<TO TEMPLATE>"
        to_template: Option<String>,
    },
}

fn existing_templates_names(file_tree: &FileTree) -> ScaffoldResult<Vec<String>> {
    let templates_path = PathBuf::new().join(templates_path());

    match dir_content(file_tree, &templates_path) {
        Ok(templates_dir_content) => {
            let templates: Vec<String> = templates_dir_content
                .into_keys()
                .map(|k| k.to_str().unwrap().to_string())
                .collect();
            Ok(templates)
        }
        _ => Ok(vec![]),
    }
}

fn choose_existing_template(file_tree: &FileTree) -> ScaffoldResult<String> {
    let templates = existing_templates_names(file_tree)?;
    match templates.len() {
        0 => Err(ScaffoldError::NoTemplatesFound),
        1 => Ok(templates[0].clone()),
        _ => {
            let option = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Which existing template should the new template be merged into?")
                .default(0)
                .items(&templates[..])
                .interact()?;

            Ok(templates[option].clone())
        }
    }
}

impl HcScaffoldTemplate {
    pub fn run(self) -> anyhow::Result<()> {
        let (template_name, template_file_tree) = self.get_template_file_tree()?;

        let target_template = match self.target_template() {
            Some(t) => t,
            None => {
                let current_dir = std::env::current_dir()?;

                let file_tree = load_directory_into_memory(&current_dir)?;

                let mut create = true;
                // If existing templates
                if existing_templates_names(&file_tree)?.len() != 0 {
                    // Merge or create?

                    let selection = Select::with_theme(&ColorfulTheme::default())
                        .with_prompt("Do you want to create a new template in this repository?")
                        .default(0)
                        .item("Merge with an existing template")
                        .item("Create a new template")
                        .interact()?;

                    if selection == 0 {
                        create = false;
                    }
                }

                if create {
                    // Enter template name
                    let template_name = Input::with_theme(&ColorfulTheme::default())
                        .with_prompt("Enter new template name:")
                        .with_initial_text(template_name)
                        .interact()?;
                    template_name
                } else {
                    let existing_template = choose_existing_template(&file_tree)?;
                    existing_template
                }
            }
        };

        let template_file_tree = dir! {
            templates_path().join(&target_template) => template_file_tree
        };

        let file_tree = MergeableFileSystemTree::<OsString, String>::from(template_file_tree);

        file_tree.build(&".".into())?;

        match self {
            HcScaffoldTemplate::Get { .. } => {
                println!(
                    r#"Template downloaded to folder {:?}
"#,
                    templates_path().join(target_template)
                );
            }
            HcScaffoldTemplate::Init { template, .. } => {
                println!(
                    r#"Template initialized to folder {:?}
"#,
                    templates_path().join(target_template)
                );
            }
        }
        Ok(())
    }

    pub fn target_template(&self) -> Option<String> {
        match self {
            HcScaffoldTemplate::Get {
                to_template: target_template,
                ..
            } => target_template.clone(),
            HcScaffoldTemplate::Init {
                to_template: target_template,
                ..
            } => target_template.clone(),
        }
    }

    pub fn get_template_file_tree(&self) -> ScaffoldResult<(String, FileTree)> {
        match self {
            HcScaffoldTemplate::Get {
                template_url,
                from_template: template,
                ..
            } => get_template(template_url, template),

            HcScaffoldTemplate::Init { template, .. } => {
                let ui_framework = match template {
                    Some(t) => t.clone(),
                    None => choose_ui_framework()?,
                };
                Ok((
                    format!("{}", ui_framework.to_string()),
                    template_for_ui_framework(&ui_framework)?,
                ))
            }
        }
    }
}
