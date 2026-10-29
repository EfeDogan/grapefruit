use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;

use file_formatter_core::{ConversionEngine, LibreOfficeConverter, TargetFormat, plan_conversion};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    let Some(command) = arguments.first().map(String::as_str) else {
        return Err(usage());
    };

    match command {
        "doctor" => doctor(),
        "capabilities" => capabilities(&arguments[1..]),
        "convert" => convert(&arguments[1..]),
        "help" | "--help" | "-h" => {
            println!("{}", usage());
            Ok(())
        }
        unknown => Err(format!("unknown command: {unknown}\n\n{}", usage())),
    }
}

fn doctor() -> Result<(), String> {
    let engine = ConversionEngine::discover();
    let providers = engine.office_provider_names();
    if providers.is_empty() {
        return Err("no compatible office conversion provider was found".to_owned());
    }
    println!("Office providers: {}", providers.join(", "));
    if let Ok(converter) = LibreOfficeConverter::discover() {
        let version = converter.version().map_err(|error| error.to_string())?;
        println!("LibreOffice: {version}");
        println!("Executable: {}", converter.executable().display());
    }
    Ok(())
}

fn capabilities(arguments: &[String]) -> Result<(), String> {
    if arguments.is_empty() {
        return Err("capabilities requires at least one file".to_owned());
    }

    let engine = ConversionEngine::discover();
    for argument in arguments {
        let path = PathBuf::from(argument);
        let targets = engine.supported_targets(&path);
        let targets = if targets.is_empty() {
            "none".to_owned()
        } else {
            targets
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        println!("{}: {targets}", path.display());
    }
    Ok(())
}

fn convert(arguments: &[String]) -> Result<(), String> {
    let mut output_directory = None;
    let mut inputs = Vec::new();
    let mut target = None;
    let mut index = 0;

    while index < arguments.len() {
        match arguments[index].as_str() {
            "--to" => {
                let value = arguments
                    .get(index + 1)
                    .ok_or_else(|| "--to requires a format".to_owned())?;
                target = Some(TargetFormat::from_str(value)?);
                index += 2;
            }
            "--output" | "-o" => {
                let value = arguments
                    .get(index + 1)
                    .ok_or_else(|| "--output requires a directory".to_owned())?;
                output_directory = Some(PathBuf::from(value));
                index += 2;
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown option: {value}"));
            }
            value => {
                inputs.push(PathBuf::from(value));
                index += 1;
            }
        }
    }

    if inputs.is_empty() {
        return Err("convert requires at least one input file".to_owned());
    }

    let target = target.ok_or_else(|| "convert requires --to FORMAT".to_owned())?;
    let plan = plan_conversion(&inputs, target, output_directory.as_deref())
        .map_err(|error| error.to_string())?;

    for job in &plan.jobs {
        println!("{} -> {}", job.input.display(), job.output.display());
    }

    let outputs = ConversionEngine::discover()
        .execute(&plan)
        .map_err(|error| error.to_string())?;
    for output in outputs {
        println!("created: {}", output.display());
    }
    Ok(())
}

fn usage() -> String {
    [
        "Grapefruit local conversion engine",
        "",
        "Usage:",
        "  file-formatter-cli doctor",
        "  file-formatter-cli capabilities <FILE>...",
        "  file-formatter-cli convert --to FORMAT [--output DIR] <FILE>...",
    ]
    .join("\n")
}
