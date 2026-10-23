use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use printpdf::{Mm, Op, PdfDocument, PdfPage, PdfSaveOptions, RawImage, XObjectTransform};

static JOB_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Creates a conversion-tool process without exposing a console window on Windows.
///
/// Grapefruit itself is a GUI application. Without `CREATE_NO_WINDOW`, console-subsystem tools
/// such as FFmpeg and Poppler create a visible Command Prompt window for every conversion.
fn background_command(program: impl AsRef<OsStr>) -> Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt as _;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut command = Command::new(program);
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new(program)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Image,
    Video,
    Pdf,
    TextDocument,
    Presentation,
    Spreadsheet,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    TextDocument,
    Presentation,
    Spreadsheet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetFormat {
    Pdf,
    Jpg,
    Png,
    Webp,
    Tiff,
    Avif,
    Mp4,
    Mov,
    Mkv,
    Webm,
    Avi,
    Gif,
    Mp3,
    Docx,
    Odt,
    Txt,
    Md,
    Html,
    Epub,
    Pptx,
    Odp,
}

impl TargetFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Jpg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Tiff => "tiff",
            Self::Avif => "avif",
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
            Self::Avi => "avi",
            Self::Gif => "gif",
            Self::Mp3 => "mp3",
            Self::Docx => "docx",
            Self::Odt => "odt",
            Self::Txt => "txt",
            Self::Md => "md",
            Self::Html => "html",
            Self::Epub => "epub",
            Self::Pptx => "pptx",
            Self::Odp => "odp",
        }
    }

    const fn is_image(self) -> bool {
        matches!(
            self,
            Self::Jpg | Self::Png | Self::Webp | Self::Tiff | Self::Avif
        )
    }

    const fn is_video_or_audio(self) -> bool {
        matches!(
            self,
            Self::Mp4 | Self::Mov | Self::Mkv | Self::Webm | Self::Avi | Self::Gif | Self::Mp3
        )
    }
}

impl fmt::Display for TargetFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.extension())
    }
}

impl FromStr for TargetFormat {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pdf" => Ok(Self::Pdf),
            "jpg" | "jpeg" => Ok(Self::Jpg),
            "png" => Ok(Self::Png),
            "webp" => Ok(Self::Webp),
            "tif" | "tiff" => Ok(Self::Tiff),
            "avif" => Ok(Self::Avif),
            "mp4" => Ok(Self::Mp4),
            "mov" => Ok(Self::Mov),
            "mkv" => Ok(Self::Mkv),
            "webm" => Ok(Self::Webm),
            "avi" => Ok(Self::Avi),
            "gif" => Ok(Self::Gif),
            "mp3" => Ok(Self::Mp3),
            "docx" => Ok(Self::Docx),
            "odt" => Ok(Self::Odt),
            "txt" => Ok(Self::Txt),
            "md" | "markdown" => Ok(Self::Md),
            "html" | "htm" => Ok(Self::Html),
            "epub" => Ok(Self::Epub),
            "pptx" => Ok(Self::Pptx),
            "odp" => Ok(Self::Odp),
            _ => Err(format!("unsupported target format: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionJob {
    pub input: PathBuf,
    pub output: PathBuf,
    pub target: TargetFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionPlan {
    pub jobs: Vec<ConversionJob>,
}

#[derive(Debug)]
pub enum ConversionError {
    NoInput,
    CurrentDirectoryUnavailable(std::io::Error),
    MissingInput(PathBuf),
    NotAFile(PathBuf),
    UnsupportedInput(PathBuf),
    UnsupportedTarget {
        input: PathBuf,
        target: TargetFormat,
    },
    InvalidFileName(PathBuf),
    OutputDirectoryUnavailable(PathBuf, std::io::Error),
    ToolNotFound(&'static str),
    ToolLaunch {
        tool: &'static str,
        source: std::io::Error,
    },
    ToolFailed {
        tool: &'static str,
        input: PathBuf,
        status: Option<i32>,
        stderr: String,
        stdout: String,
    },
    NoAudioStream(PathBuf),
    OutputNotProduced {
        input: PathBuf,
        expected: PathBuf,
        stdout: String,
        stderr: String,
    },
    OutputAlreadyExists(PathBuf),
    FinalizeOutput {
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
    PdfWrite {
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for ConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInput => formatter.write_str("no input files were provided"),
            Self::CurrentDirectoryUnavailable(error) => write!(
                formatter,
                "could not resolve the current directory: {error}"
            ),
            Self::MissingInput(path) => {
                write!(formatter, "input does not exist: {}", path.display())
            }
            Self::NotAFile(path) => {
                write!(formatter, "input is not a regular file: {}", path.display())
            }
            Self::UnsupportedInput(path) => {
                write!(formatter, "unsupported input format: {}", path.display())
            }
            Self::UnsupportedTarget { input, target } => write!(
                formatter,
                "{} cannot be converted to {target}",
                input.display()
            ),
            Self::InvalidFileName(path) => write!(
                formatter,
                "input has no usable file name: {}",
                path.display()
            ),
            Self::OutputDirectoryUnavailable(path, error) => write!(
                formatter,
                "output directory is unavailable ({}): {error}",
                path.display()
            ),
            Self::ToolNotFound(tool) => write!(
                formatter,
                "{tool} was not found; install it or configure its FILE_FORMATTER_* environment variable"
            ),
            Self::ToolLaunch { tool, source } => {
                write!(formatter, "could not start {tool}: {source}")
            }
            Self::ToolFailed {
                tool,
                input,
                status,
                stderr,
                stdout,
            } => write!(
                formatter,
                "{tool} failed for {} (exit {:?})\nstdout: {}\nstderr: {}",
                input.display(),
                status,
                stdout.trim(),
                stderr.trim()
            ),
            Self::NoAudioStream(path) => write!(
                formatter,
                "no audio stream was found in {}; MP3 export requires a video with audio",
                path.display()
            ),
            Self::OutputNotProduced {
                input,
                expected,
                stdout,
                stderr,
            } => write!(
                formatter,
                "conversion produced no output for {} (expected {})\nstdout: {}\nstderr: {}",
                input.display(),
                expected.display(),
                stdout.trim(),
                stderr.trim()
            ),
            Self::OutputAlreadyExists(path) => write!(
                formatter,
                "refusing to overwrite output: {}",
                path.display()
            ),
            Self::FinalizeOutput { from, to, source } => write!(
                formatter,
                "could not finalize output from {} to {}: {source}",
                from.display(),
                to.display()
            ),
            Self::PdfWrite { path, message } => write!(
                formatter,
                "could not create PDF {}: {message}",
                path.display()
            ),
        }
    }
}

impl Error for ConversionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CurrentDirectoryUnavailable(error)
            | Self::OutputDirectoryUnavailable(_, error) => Some(error),
            Self::ToolLaunch { source, .. } | Self::FinalizeOutput { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn file_kind(path: &Path) -> FileKind {
    let Some(extension) = normalized_extension(path) else {
        return FileKind::Unsupported;
    };
    match extension.as_str() {
        "jpg" | "jpeg" | "png" | "webp" | "tif" | "tiff" | "avif" | "bmp" | "heic" | "heif" => {
            FileKind::Image
        }
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "wmv" | "gif" => FileKind::Video,
        "pdf" => FileKind::Pdf,
        "doc" | "docx" | "odt" | "rtf" | "txt" | "html" | "htm" | "md" | "markdown" | "epub" => {
            FileKind::TextDocument
        }
        "ppt" | "pptx" | "odp" => FileKind::Presentation,
        "xls" | "xlsx" | "ods" => FileKind::Spreadsheet,
        _ => FileKind::Unsupported,
    }
}

pub fn document_kind(path: &Path) -> Option<DocumentKind> {
    match file_kind(path) {
        FileKind::TextDocument => Some(DocumentKind::TextDocument),
        FileKind::Presentation => Some(DocumentKind::Presentation),
        FileKind::Spreadsheet => Some(DocumentKind::Spreadsheet),
        _ => None,
    }
}

pub fn supported_targets(path: &Path) -> Vec<TargetFormat> {
    let candidates: &[TargetFormat] = match file_kind(path) {
        FileKind::Image => &[
            TargetFormat::Jpg,
            TargetFormat::Png,
            TargetFormat::Webp,
            TargetFormat::Tiff,
            TargetFormat::Avif,
            TargetFormat::Pdf,
        ],
        FileKind::Video => &[
            TargetFormat::Mp4,
            TargetFormat::Mov,
            TargetFormat::Mkv,
            TargetFormat::Webm,
            TargetFormat::Gif,
            TargetFormat::Mp3,
            TargetFormat::Avi,
        ],
        FileKind::Pdf => &[
            TargetFormat::Png,
            TargetFormat::Jpg,
            TargetFormat::Txt,
            TargetFormat::Md,
        ],
        FileKind::TextDocument => &[
            TargetFormat::Pdf,
            TargetFormat::Docx,
            TargetFormat::Odt,
            TargetFormat::Txt,
            TargetFormat::Html,
            TargetFormat::Epub,
        ],
        FileKind::Presentation => &[
            TargetFormat::Pdf,
            TargetFormat::Pptx,
            TargetFormat::Odp,
            TargetFormat::Png,
            TargetFormat::Jpg,
        ],
        FileKind::Spreadsheet => &[TargetFormat::Pdf],
        FileKind::Unsupported => &[],
    };
    let source = source_target_format(path);
    candidates
        .iter()
        .copied()
        .filter(|candidate| Some(*candidate) != source)
        .collect()
}

pub fn plan_conversion(
    inputs: &[PathBuf],
    target: TargetFormat,
    output_directory: Option<&Path>,
) -> Result<ConversionPlan, ConversionError> {
    if inputs.is_empty() {
        return Err(ConversionError::NoInput);
    }
    let current_directory =
        env::current_dir().map_err(ConversionError::CurrentDirectoryUnavailable)?;
    let output_directory = output_directory.map(|path| absolute_path(path, &current_directory));
    if let Some(directory) = output_directory.as_deref() {
        ensure_output_directory(directory)?;
    }

    let mut reserved_outputs = Vec::with_capacity(inputs.len());
    let mut jobs = Vec::with_capacity(inputs.len());
    for input in inputs {
        let input = absolute_path(input, &current_directory);
        let metadata =
            fs::metadata(&input).map_err(|_| ConversionError::MissingInput(input.clone()))?;
        if !metadata.is_file() {
            return Err(ConversionError::NotAFile(input));
        }
        if file_kind(&input) == FileKind::Unsupported {
            return Err(ConversionError::UnsupportedInput(input));
        }
        if !supported_targets(&input).contains(&target) {
            return Err(ConversionError::UnsupportedTarget { input, target });
        }
        let file_stem = input
            .file_stem()
            .filter(|stem| !stem.is_empty())
            .ok_or_else(|| ConversionError::InvalidFileName(input.clone()))?;
        let parent = output_directory.as_deref().map_or_else(
            || {
                input
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf()
            },
            Path::to_path_buf,
        );
        ensure_output_directory(&parent)?;
        let output = unique_output_path(&parent, file_stem, target.extension(), &reserved_outputs);
        reserved_outputs.push(output.clone());
        jobs.push(ConversionJob {
            input,
            output,
            target,
        });
    }
    Ok(ConversionPlan { jobs })
}

pub fn plan_pdf_conversion(
    inputs: &[PathBuf],
    output_directory: Option<&Path>,
) -> Result<ConversionPlan, ConversionError> {
    plan_conversion(inputs, TargetFormat::Pdf, output_directory)
}

#[derive(Debug, Clone, Default)]
pub struct ConversionEngine {
    libreoffice: Option<PathBuf>,
    ffmpeg: Option<PathBuf>,
    pdftoppm: Option<PathBuf>,
    pdftotext: Option<PathBuf>,
}

impl ConversionEngine {
    pub fn discover() -> Self {
        let ffmpeg_fallbacks = bundled_tool_fallbacks(&["tools/ffmpeg/ffmpeg.exe"]);
        let pdftoppm_fallbacks = bundled_tool_fallbacks(&["tools/poppler/bin/pdftoppm.exe"]);
        let pdftotext_fallbacks = bundled_tool_fallbacks(&["tools/poppler/bin/pdftotext.exe"]);
        Self {
            libreoffice: discover_libreoffice(),
            ffmpeg: discover_tool("FILE_FORMATTER_FFMPEG", &["ffmpeg"], &ffmpeg_fallbacks),
            pdftoppm: discover_tool(
                "FILE_FORMATTER_PDFTOPPM",
                &["pdftoppm"],
                &pdftoppm_fallbacks,
            ),
            pdftotext: discover_tool(
                "FILE_FORMATTER_PDFTOTEXT",
                &["pdftotext"],
                &pdftotext_fallbacks,
            ),
        }
    }

    pub fn execute(&self, plan: &ConversionPlan) -> Result<Vec<PathBuf>, ConversionError> {
        let mut completed = Vec::new();
        for job in &plan.jobs {
            completed.extend(self.execute_job(job)?);
        }
        Ok(completed)
    }

    fn execute_job(&self, job: &ConversionJob) -> Result<Vec<PathBuf>, ConversionError> {
        match (file_kind(&job.input), job.target) {
            (FileKind::Image, TargetFormat::Pdf) => self.image_to_pdf(job),
            (FileKind::Image, target) if target.is_image() => self.ffmpeg_single(job),
            (FileKind::Video, target) if target.is_video_or_audio() => self.ffmpeg_single(job),
            (FileKind::Pdf, TargetFormat::Png | TargetFormat::Jpg) => self.pdf_to_images(job),
            (FileKind::Pdf, TargetFormat::Txt | TargetFormat::Md) => self.pdf_to_text(job),
            (FileKind::Presentation, TargetFormat::Png | TargetFormat::Jpg) => {
                self.presentation_to_images(job)
            }
            (FileKind::TextDocument | FileKind::Presentation | FileKind::Spreadsheet, _) => {
                self.libreoffice_single(job)
            }
            _ => Err(ConversionError::UnsupportedTarget {
                input: job.input.clone(),
                target: job.target,
            }),
        }
    }

    fn ffmpeg(&self) -> Result<&Path, ConversionError> {
        self.ffmpeg
            .as_deref()
            .ok_or(ConversionError::ToolNotFound("FFmpeg"))
    }
    fn libreoffice(&self) -> Result<&Path, ConversionError> {
        self.libreoffice
            .as_deref()
            .ok_or(ConversionError::ToolNotFound("LibreOffice"))
    }
    fn pdftoppm(&self) -> Result<&Path, ConversionError> {
        self.pdftoppm
            .as_deref()
            .ok_or(ConversionError::ToolNotFound("pdftoppm (Poppler)"))
    }
    fn pdftotext(&self) -> Result<&Path, ConversionError> {
        self.pdftotext
            .as_deref()
            .ok_or(ConversionError::ToolNotFound("pdftotext (Poppler)"))
    }

    fn ffmpeg_single(&self, job: &ConversionJob) -> Result<Vec<PathBuf>, ConversionError> {
        let staging =
            unique_staging_directory(job.output.parent().unwrap_or_else(|| Path::new(".")));
        let result = (|| {
            fs::create_dir_all(&staging).map_err(|error| {
                ConversionError::OutputDirectoryUnavailable(staging.clone(), error)
            })?;
            let generated = staging.join(format!("output.{}", job.target.extension()));
            run_ffmpeg(self.ffmpeg()?, &job.input, job.target, &generated)?;
            finalize_single(&generated, &job.output).map(|path| vec![path])
        })();
        let _ = fs::remove_dir_all(&staging);
        result
    }

    fn image_to_pdf(&self, job: &ConversionJob) -> Result<Vec<PathBuf>, ConversionError> {
        let staging =
            unique_staging_directory(job.output.parent().unwrap_or_else(|| Path::new(".")));
        let result = (|| {
            fs::create_dir_all(&staging).map_err(|error| {
                ConversionError::OutputDirectoryUnavailable(staging.clone(), error)
            })?;
            let png = staging.join("image.png");
            run_ffmpeg(self.ffmpeg()?, &job.input, TargetFormat::Png, &png)?;
            let image_bytes = fs::read(&png).map_err(|error| ConversionError::PdfWrite {
                path: png.clone(),
                message: error.to_string(),
            })?;
            let mut warnings = Vec::new();
            let image =
                RawImage::decode_from_bytes(&image_bytes, &mut warnings).map_err(|message| {
                    ConversionError::PdfWrite {
                        path: job.output.clone(),
                        message,
                    }
                })?;
            let page_width = Mm(image.width as f32 * 25.4 / 300.0);
            let page_height = Mm(image.height as f32 * 25.4 / 300.0);
            let mut document = PdfDocument::new("Grapefruit image export");
            let image_id = document.add_image(&image);
            let page = PdfPage::new(
                page_width,
                page_height,
                vec![Op::UseXobject {
                    id: image_id,
                    transform: XObjectTransform {
                        dpi: Some(300.0),
                        ..Default::default()
                    },
                }],
            );
            let bytes = document
                .with_pages(vec![page])
                .save(&PdfSaveOptions::default(), &mut warnings);
            let generated = staging.join("output.pdf");
            fs::write(&generated, bytes).map_err(|error| ConversionError::PdfWrite {
                path: generated.clone(),
                message: error.to_string(),
            })?;
            finalize_single(&generated, &job.output).map(|path| vec![path])
        })();
        let _ = fs::remove_dir_all(&staging);
        result
    }

    fn pdf_to_images(&self, job: &ConversionJob) -> Result<Vec<PathBuf>, ConversionError> {
        let staging =
            unique_staging_directory(job.output.parent().unwrap_or_else(|| Path::new(".")));
        let result = (|| {
            fs::create_dir_all(&staging).map_err(|error| {
                ConversionError::OutputDirectoryUnavailable(staging.clone(), error)
            })?;
            let generated = rasterize_pdf(self.pdftoppm()?, &job.input, job.target, &staging)?;
            finalize_pages(&generated, &job.output)
        })();
        let _ = fs::remove_dir_all(&staging);
        result
    }

    fn pdf_to_text(&self, job: &ConversionJob) -> Result<Vec<PathBuf>, ConversionError> {
        let staging =
            unique_staging_directory(job.output.parent().unwrap_or_else(|| Path::new(".")));
        let result = (|| {
            fs::create_dir_all(&staging).map_err(|error| {
                ConversionError::OutputDirectoryUnavailable(staging.clone(), error)
            })?;
            let generated = staging.join(format!("output.{}", job.target.extension()));
            let output = background_command(self.pdftotext()?)
                .arg("-layout")
                .arg(&job.input)
                .arg(&generated)
                .output()
                .map_err(|source| ConversionError::ToolLaunch {
                    tool: "pdftotext (Poppler)",
                    source,
                })?;
            ensure_tool_success("pdftotext (Poppler)", &job.input, &output)?;
            ensure_generated(&job.input, &generated, &output)?;
            finalize_single(&generated, &job.output).map(|path| vec![path])
        })();
        let _ = fs::remove_dir_all(&staging);
        result
    }

    fn presentation_to_images(&self, job: &ConversionJob) -> Result<Vec<PathBuf>, ConversionError> {
        let staging =
            unique_staging_directory(job.output.parent().unwrap_or_else(|| Path::new(".")));
        let result = (|| {
            let generated_dir = staging.join("office");
            fs::create_dir_all(&generated_dir).map_err(|error| {
                ConversionError::OutputDirectoryUnavailable(generated_dir.clone(), error)
            })?;
            let pdf = run_libreoffice(
                self.libreoffice()?,
                &job.input,
                TargetFormat::Pdf,
                &staging,
                &generated_dir,
            )?;
            let raster_dir = staging.join("pages");
            fs::create_dir_all(&raster_dir).map_err(|error| {
                ConversionError::OutputDirectoryUnavailable(raster_dir.clone(), error)
            })?;
            let generated = rasterize_pdf(self.pdftoppm()?, &pdf, job.target, &raster_dir)?;
            finalize_pages(&generated, &job.output)
        })();
        let _ = fs::remove_dir_all(&staging);
        result
    }

    fn libreoffice_single(&self, job: &ConversionJob) -> Result<Vec<PathBuf>, ConversionError> {
        convert_with_libreoffice(self.libreoffice()?, job).map(|path| vec![path])
    }
}

#[derive(Debug, Clone)]
pub struct LibreOfficeConverter {
    executable: PathBuf,
}

impl LibreOfficeConverter {
    pub fn discover() -> Result<Self, ConversionError> {
        discover_libreoffice()
            .map(|executable| Self { executable })
            .ok_or(ConversionError::ToolNotFound("LibreOffice"))
    }
    pub fn new(executable: PathBuf) -> Self {
        Self { executable }
    }
    pub fn executable(&self) -> &Path {
        &self.executable
    }
    pub fn version(&self) -> Result<String, ConversionError> {
        let output = background_command(&self.executable)
            .arg("--version")
            .output()
            .map_err(|source| ConversionError::ToolLaunch {
                tool: "LibreOffice",
                source,
            })?;
        ensure_tool_success("LibreOffice", Path::new("--version"), &output)?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
    pub fn convert(&self, plan: &ConversionPlan) -> Result<Vec<PathBuf>, ConversionError> {
        let mut completed = Vec::with_capacity(plan.jobs.len());
        for job in &plan.jobs {
            completed.push(convert_with_libreoffice(&self.executable, job)?);
        }
        Ok(completed)
    }
}

fn convert_with_libreoffice(
    executable: &Path,
    job: &ConversionJob,
) -> Result<PathBuf, ConversionError> {
    let staging = unique_staging_directory(job.output.parent().unwrap_or_else(|| Path::new(".")));
    let result = (|| {
        let generated_dir = staging.join("output");
        fs::create_dir_all(&generated_dir).map_err(|error| {
            ConversionError::OutputDirectoryUnavailable(generated_dir.clone(), error)
        })?;
        let generated =
            run_libreoffice(executable, &job.input, job.target, &staging, &generated_dir)?;
        finalize_single(&generated, &job.output)
    })();
    let _ = fs::remove_dir_all(&staging);
    result
}

fn run_libreoffice(
    executable: &Path,
    input: &Path,
    target: TargetFormat,
    staging: &Path,
    generated_dir: &Path,
) -> Result<PathBuf, ConversionError> {
    let profile = staging.join("profile");
    fs::create_dir_all(&profile)
        .map_err(|error| ConversionError::OutputDirectoryUnavailable(profile.clone(), error))?;
    let output = background_command(executable)
        .args([
            "--headless",
            "--nologo",
            "--nodefault",
            "--nofirststartwizard",
            "--nolockcheck",
        ])
        .arg(format!("-env:UserInstallation={}", file_url(&profile)))
        .arg("--convert-to")
        .arg(libreoffice_filter(target))
        .arg("--outdir")
        .arg(generated_dir)
        .arg(input)
        .output()
        .map_err(|source| ConversionError::ToolLaunch {
            tool: "LibreOffice",
            source,
        })?;
    ensure_tool_success("LibreOffice", input, &output)?;
    let generated = generated_dir
        .join(
            input
                .file_name()
                .ok_or_else(|| ConversionError::InvalidFileName(input.to_path_buf()))?,
        )
        .with_extension(target.extension());
    ensure_generated(input, &generated, &output)?;
    Ok(generated)
}

fn libreoffice_filter(target: TargetFormat) -> &'static str {
    match target {
        TargetFormat::Html => "html:XHTML Writer File:UTF8",
        TargetFormat::Txt => "txt:Text (encoded):UTF8",
        _ => target.extension(),
    }
}

fn run_ffmpeg(
    executable: &Path,
    input: &Path,
    target: TargetFormat,
    output_path: &Path,
) -> Result<(), ConversionError> {
    let mut command = background_command(executable);
    command
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
        .arg("-i")
        .arg(input);
    match target {
        TargetFormat::Jpg => {
            command.args(["-map", "0:v:0", "-frames:v", "1", "-q:v", "2"]);
        }
        TargetFormat::Png => {
            command.args(["-map", "0:v:0", "-frames:v", "1"]);
        }
        TargetFormat::Webp => {
            command.args([
                "-map",
                "0:v:0",
                "-frames:v",
                "1",
                "-c:v",
                "libwebp",
                "-quality",
                "90",
            ]);
        }
        TargetFormat::Tiff => {
            command.args([
                "-map",
                "0:v:0",
                "-frames:v",
                "1",
                "-compression_algo",
                "deflate",
            ]);
        }
        TargetFormat::Avif => {
            command.args([
                "-map",
                "0:v:0",
                "-frames:v",
                "1",
                "-c:v",
                "libaom-av1",
                "-still-picture",
                "1",
                "-crf",
                "28",
            ]);
        }
        TargetFormat::Mp4 | TargetFormat::Mov => {
            command.args([
                "-map",
                "0:v:0?",
                "-map",
                "0:a:0?",
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "22",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-movflags",
                "+faststart",
            ]);
        }
        TargetFormat::Mkv => {
            command.args([
                "-map", "0:v:0?", "-map", "0:a:0?", "-c:v", "libx264", "-preset", "medium", "-crf",
                "22", "-c:a", "aac", "-b:a", "192k",
            ]);
        }
        TargetFormat::Webm => {
            command.args([
                "-map",
                "0:v:0?",
                "-map",
                "0:a:0?",
                "-c:v",
                "libvpx-vp9",
                "-deadline",
                "realtime",
                "-cpu-used",
                "6",
                "-row-mt",
                "1",
                "-tile-columns",
                "2",
                "-crf",
                "32",
                "-b:v",
                "0",
                "-c:a",
                "libopus",
                "-b:a",
                "160k",
            ]);
        }
        TargetFormat::Avi => {
            command.args([
                "-map",
                "0:v:0?",
                "-map",
                "0:a:0?",
                "-c:v",
                "mpeg4",
                "-q:v",
                "4",
                "-c:a",
                "libmp3lame",
                "-q:a",
                "3",
            ]);
        }
        TargetFormat::Gif => {
            command.args(["-vf", "fps=12,scale='min(960,iw)':-2:flags=lanczos", "-an"]);
        }
        TargetFormat::Mp3 => {
            command.args(["-map", "0:a:0", "-vn", "-c:a", "libmp3lame", "-q:a", "2"]);
        }
        _ => {
            return Err(ConversionError::UnsupportedTarget {
                input: input.to_path_buf(),
                target,
            });
        }
    }
    let output =
        command
            .arg(output_path)
            .output()
            .map_err(|source| ConversionError::ToolLaunch {
                tool: "FFmpeg",
                source,
            })?;
    if target == TargetFormat::Mp3
        && !output.status.success()
        && is_missing_audio_error(&output.stderr)
    {
        return Err(ConversionError::NoAudioStream(input.to_path_buf()));
    }
    ensure_tool_success("FFmpeg", input, &output)?;
    ensure_generated(input, output_path, &output)
}

fn is_missing_audio_error(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr);
    stderr.contains("matches no streams") || stderr.contains("does not contain any stream")
}

fn rasterize_pdf(
    executable: &Path,
    input: &Path,
    target: TargetFormat,
    output_dir: &Path,
) -> Result<Vec<PathBuf>, ConversionError> {
    let prefix = output_dir.join("page");
    let mut command = background_command(executable);
    command.arg("-r").arg("300");
    match target {
        TargetFormat::Png => {
            command.arg("-png");
        }
        TargetFormat::Jpg => {
            command.args(["-jpeg", "-jpegopt", "quality=92"]);
        }
        _ => {
            return Err(ConversionError::UnsupportedTarget {
                input: input.to_path_buf(),
                target,
            });
        }
    }
    let output =
        command
            .arg(input)
            .arg(&prefix)
            .output()
            .map_err(|source| ConversionError::ToolLaunch {
                tool: "pdftoppm (Poppler)",
                source,
            })?;
    ensure_tool_success("pdftoppm (Poppler)", input, &output)?;
    let mut pages = fs::read_dir(output_dir)
        .map_err(|error| {
            ConversionError::OutputDirectoryUnavailable(output_dir.to_path_buf(), error)
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(OsStr::to_str) == Some(target.extension()))
        .collect::<Vec<_>>();
    pages.sort_by_key(|path| page_number(path));
    if pages.is_empty() {
        return Err(ConversionError::OutputNotProduced {
            input: input.to_path_buf(),
            expected: prefix.with_extension(target.extension()),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(pages)
}

fn page_number(path: &Path) -> u32 {
    path.file_stem()
        .and_then(OsStr::to_str)
        .and_then(|stem| stem.rsplit('-').next())
        .and_then(|number| number.parse().ok())
        .unwrap_or(u32::MAX)
}

fn finalize_pages(
    generated: &[PathBuf],
    nominal_output: &Path,
) -> Result<Vec<PathBuf>, ConversionError> {
    if generated.len() == 1 {
        return finalize_single(&generated[0], nominal_output).map(|path| vec![path]);
    }
    let directory = nominal_output.parent().unwrap_or_else(|| Path::new("."));
    let stem = nominal_output
        .file_stem()
        .ok_or_else(|| ConversionError::InvalidFileName(nominal_output.to_path_buf()))?
        .to_string_lossy();
    let extension = nominal_output
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default();
    for suffix in 0_u32.. {
        let group = if suffix == 0 {
            stem.to_string()
        } else {
            format!("{stem} ({suffix})")
        };
        let targets = (1..=generated.len())
            .map(|page| directory.join(format!("{group}-{page}.{extension}")))
            .collect::<Vec<_>>();
        if targets.iter().any(|path| path.exists()) {
            continue;
        }
        for (from, to) in generated.iter().zip(&targets) {
            move_completed_output(from, to)?;
        }
        return Ok(targets);
    }
    unreachable!("the output suffix space is not practically exhaustible")
}

fn finalize_single(generated: &Path, output: &Path) -> Result<PathBuf, ConversionError> {
    if output.exists() {
        return Err(ConversionError::OutputAlreadyExists(output.to_path_buf()));
    }
    move_completed_output(generated, output)?;
    Ok(output.to_path_buf())
}

fn move_completed_output(from: &Path, to: &Path) -> Result<(), ConversionError> {
    if to.exists() {
        return Err(ConversionError::OutputAlreadyExists(to.to_path_buf()));
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }

    // The system temp directory can live on another drive. Copy only after the converter has
    // completed, using create_new so a file that appeared in the meantime is never overwritten.
    let mut source = fs::File::open(from).map_err(|source| ConversionError::FinalizeOutput {
        from: from.to_path_buf(),
        to: to.to_path_buf(),
        source,
    })?;
    let mut destination = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
        .map_err(|source| ConversionError::FinalizeOutput {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            source,
        })?;
    let result = io::copy(&mut source, &mut destination).and_then(|_| destination.sync_all());
    if let Err(source) = result {
        drop(destination);
        let _ = fs::remove_file(to);
        return Err(ConversionError::FinalizeOutput {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            source,
        });
    }
    Ok(())
}

fn ensure_tool_success(
    tool: &'static str,
    input: &Path,
    output: &Output,
) -> Result<(), ConversionError> {
    if output.status.success() {
        return Ok(());
    }
    Err(ConversionError::ToolFailed {
        tool,
        input: input.to_path_buf(),
        status: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
    })
}

fn ensure_generated(input: &Path, expected: &Path, output: &Output) -> Result<(), ConversionError> {
    if expected.is_file() {
        return Ok(());
    }
    Err(ConversionError::OutputNotProduced {
        input: input.to_path_buf(),
        expected: expected.to_path_buf(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn source_target_format(path: &Path) -> Option<TargetFormat> {
    normalized_extension(path).and_then(|extension| TargetFormat::from_str(&extension).ok())
}
fn normalized_extension(path: &Path) -> Option<String> {
    path.extension()?.to_str().map(str::to_ascii_lowercase)
}
fn absolute_path(path: &Path, current_directory: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_directory.join(path)
    }
}

fn ensure_output_directory(path: &Path) -> Result<(), ConversionError> {
    let metadata = fs::metadata(path)
        .map_err(|error| ConversionError::OutputDirectoryUnavailable(path.to_path_buf(), error))?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(ConversionError::OutputDirectoryUnavailable(
            path.to_path_buf(),
            std::io::Error::new(std::io::ErrorKind::NotADirectory, "not a directory"),
        ))
    }
}

fn unique_output_path(
    directory: &Path,
    file_stem: &OsStr,
    extension: &str,
    reserved: &[PathBuf],
) -> PathBuf {
    let stem = file_stem.to_string_lossy();
    for index in 0_u32.. {
        let name = if index == 0 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem} ({index}).{extension}")
        };
        let candidate = directory.join(name);
        if !candidate.exists() && !reserved.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("the output suffix space is not practically exhaustible")
}

pub fn discover_libreoffice() -> Option<PathBuf> {
    let fallbacks = vec![
        PathBuf::from("/usr/bin/soffice"),
        PathBuf::from("/usr/bin/libreoffice"),
        PathBuf::from("/snap/bin/libreoffice"),
    ];
    #[cfg(target_os = "windows")]
    let mut fallbacks = fallbacks;
    #[cfg(target_os = "windows")]
    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(base) = env::var_os(variable) {
            fallbacks.push(
                PathBuf::from(base)
                    .join("LibreOffice")
                    .join("program")
                    .join("soffice.exe"),
            );
        }
    }
    discover_tool(
        "FILE_FORMATTER_LIBREOFFICE",
        &["soffice", "libreoffice"],
        &fallbacks,
    )
}

fn discover_tool(variable: &str, names: &[&str], fallbacks: &[PathBuf]) -> Option<PathBuf> {
    if let Some(configured) = env::var_os(variable) {
        let configured = PathBuf::from(configured);
        if configured.is_file() {
            return Some(configured);
        }
    }
    for name in names {
        if let Some(found) = find_on_path(name) {
            return Some(found);
        }
    }
    fallbacks
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
}

fn bundled_tool_fallbacks(relative_paths: &[&str]) -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let Some(executable_directory) = env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
        else {
            return Vec::new();
        };
        return relative_paths
            .iter()
            .map(|relative| executable_directory.join(relative))
            .collect();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = relative_paths;
        Vec::new()
    }
}

fn find_on_path(executable: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for directory in env::split_paths(&path) {
        let candidate = directory.join(executable);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(target_os = "windows")]
        {
            let candidate = directory.join(format!("{executable}.exe"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn unique_staging_directory(_output_parent: &Path) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = JOB_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join("grapefruit").join(format!(
        ".file-formatter-{}-{timestamp}-{sequence}",
        std::process::id()
    ))
}

fn file_url(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let encoded = percent_encode_path(&normalized);
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

fn percent_encode_path(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/' | b':') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new(name: &str) -> Self {
            let sequence = JOB_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = env::temp_dir().join(format!(
                "file-formatter-tests-{}-{name}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn touch(path: &Path) {
        fs::write(path, b"test fixture").expect("create fixture");
    }

    #[test]
    fn recognizes_all_priority_families_case_insensitively() {
        assert_eq!(file_kind(Path::new("photo.PNG")), FileKind::Image);
        assert_eq!(file_kind(Path::new("clip.MKV")), FileKind::Video);
        assert_eq!(file_kind(Path::new("manual.PDF")), FileKind::Pdf);
        assert_eq!(file_kind(Path::new("report.DOCX")), FileKind::TextDocument);
        assert_eq!(file_kind(Path::new("slides.PPTX")), FileKind::Presentation);
    }

    #[test]
    fn excludes_the_source_format_from_targets() {
        let targets = supported_targets(Path::new("photo.png"));
        assert!(!targets.contains(&TargetFormat::Png));
        assert!(targets.contains(&TargetFormat::Jpg));
        assert!(targets.contains(&TargetFormat::Pdf));
    }

    #[test]
    fn keeps_conversion_staging_outside_the_output_directory() {
        let output = TestDirectory::new("output");
        let staging = unique_staging_directory(output.path());
        assert!(staging.starts_with(env::temp_dir().join("grapefruit")));
        assert!(!staging.starts_with(output.path()));
    }

    #[test]
    fn plans_requested_targets_without_overwrite() {
        let directory = TestDirectory::new("targets");
        let input = directory.path().join("photo.png");
        touch(&input);
        touch(&directory.path().join("photo.webp"));
        let plan = plan_conversion(&[input], TargetFormat::Webp, None).expect("plan conversion");
        assert_eq!(plan.jobs[0].output, directory.path().join("photo (1).webp"));
    }

    #[test]
    fn plans_batch_pdf_outputs_beside_documents() {
        let directory = TestDirectory::new("batch");
        let document = directory.path().join("report.docx");
        let slides = directory.path().join("slides.pptx");
        touch(&document);
        touch(&slides);
        let plan = plan_pdf_conversion(&[document, slides], None).expect("plan conversion");
        assert_eq!(plan.jobs[0].output, directory.path().join("report.pdf"));
        assert_eq!(plan.jobs[1].output, directory.path().join("slides.pdf"));
    }

    #[test]
    fn rejects_invalid_target_for_source() {
        let directory = TestDirectory::new("unsupported");
        let input = directory.path().join("photo.png");
        touch(&input);
        let error = plan_conversion(std::slice::from_ref(&input), TargetFormat::Mp4, None)
            .expect_err("reject target");
        assert!(
            matches!(error, ConversionError::UnsupportedTarget { input: path, target: TargetFormat::Mp4 } if path == input)
        );
    }

    #[test]
    fn creates_valid_file_url_for_profile_with_spaces() {
        assert_eq!(
            file_url(Path::new("/tmp/Grapefruit App/profile")),
            "file:///tmp/Grapefruit%20App/profile"
        );
    }

    #[test]
    fn recognizes_ffmpeg_missing_audio_errors() {
        assert!(is_missing_audio_error(
            b"Stream map '0:a:0' matches no streams."
        ));
        assert!(is_missing_audio_error(
            b"Output file does not contain any stream"
        ));
        assert!(!is_missing_audio_error(b"Encoder not found"));
    }
}
