//! OPCA CLI — Command-line interface for the `OpenPilot` Code Agent.
//!
//! Non-blocking, background-first code agent:
//! - Dispatch tasks to background workers
//! - Keep chatting while tasks run
//! - Never block the foreground

#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use opca_cli::{MockOrchestrator, OrchestratorApi, RealOrchestrator, StdOutput};
use opca_core::provider::Provider;
use opca_core::session::SESSIONS_DIR;

/// Default model used when `--model` is not supplied and no env override is set.
const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";

/// `opca` — background-first code agent.
///
/// Dispatch long-running work to background Tasks, keep chatting in the
/// foreground, never block on a single agent turn. Run `opca --help` for
/// options or type `/help` once inside the REPL for slash commands.
#[derive(Parser, Debug)]
#[command(
    name = "opca",
    version,
    about = "Background-first code agent — dispatch tasks, keep chatting",
    long_about = "Background-first code agent.\n\n\
        Dispatch long work to background Tasks and keep chatting in the\n\
        foreground. Each Task runs in its own isolated workspace; the\n\
        Orchestrator routes messages, aggregates heartbeats, and audits\n\
        finished work before merging it back."
)]
struct Cli {
    /// Project path to operate on (defaults to current directory).
    #[arg(
        long,
        value_name = "PATH",
        default_value = ".",
        help = "Project path to operate on (defaults to current directory)"
    )]
    project: PathBuf,

    /// Session ID to resume (creates a new session when omitted).
    #[arg(
        long,
        value_name = "ID",
        help = "Session ID to resume (creates a new session if omitted)"
    )]
    session: Option<String>,

    /// Model identifier passed to the configured provider
    /// (e.g. `claude-sonnet-4-20250514`, `gpt-4o`, `gemini-1.5-pro`).
    /// Falls back to the `OPCA_MODEL` environment variable, then `DEFAULT_MODEL`.
    #[arg(
        long,
        value_name = "MODEL",
        help = "Model identifier passed to the provider"
    )]
    model: Option<String>,

    /// Provider kind: zhipu, deepseek, openai, anthropic, ollama, moonshot,
    /// groq, mistral, openrouter, gemini. Overrides config and model-based
    /// inference.
    #[arg(
        long = "provider",
        value_name = "KIND",
        help = "Provider kind: zhipu, deepseek, openai, anthropic, ollama, ..."
    )]
    provider_kind: Option<String>,

    /// Verbose logging. Pass once for `info`, twice (`-vv`) for `debug`,
    /// three times for `trace`.
    #[arg(short, long, action = clap::ArgAction::Count, help = "Verbose logging")]
    verbose: u8,

    /// Use a mock orchestrator (no LLM, for demos and smoke testing).
    #[arg(
        long,
        default_value_t = false,
        help = "Use a mock orchestrator (no LLM)"
    )]
    mock: bool,

    /// Disable the pending-review indicator in the prompt.
    #[arg(
        long,
        default_value_t = false,
        help = "Disable the pending-review indicator in the prompt"
    )]
    no_review_indicator: bool,
}

fn main() {
    let args = Cli::parse();

    if !args.project.exists() {
        eprintln!(
            "error: project path does not exist: {}",
            args.project.display()
        );
        std::process::exit(1);
    }

    let config = opca_core::config::Config::load(&args.project);

    let model = args
        .model
        .clone()
        .or_else(|| std::env::var("OPCA_MODEL").ok())
        .or_else(|| config.model.default.clone())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    init_tracing(args.verbose);

    let rt = tokio::runtime::Runtime::new().expect("failed to init tokio runtime");
    rt.block_on(async move {
        let _output: Arc<dyn opca_cli::repl::Output> = Arc::new(StdOutput);
        let orchestrator: Arc<dyn OrchestratorApi> = if args.mock {
            Arc::new(MockOrchestrator::new())
        } else if let Some(p) = create_provider(&args, &model, &config) {
            Arc::new(RealOrchestrator::with_std_di(p, args.project.clone()))
        } else {
            eprintln!("error: no API key found. Set one of:");
            eprintln!("  ANTHROPIC_API_KEY, OPENAI_API_KEY, ZHIPU_API_KEY, DEEPSEEK_API_KEY, ...");
            eprintln!("  Or use --mock for a demo without LLM.");
            std::process::exit(1);
        };

        if args.mock {
            let output: Arc<dyn opca_cli::repl::Output> = Arc::new(StdOutput);
            let mut runtime =
                opca_cli::repl::ReplRuntime::run(orchestrator, output, !args.no_review_indicator);
            if let Some(handle) = runtime.repl_handle.take() {
                let _ = handle.await;
            }
            runtime.shutdown();
        } else {
            run_tui(orchestrator, model).await;
        }
    });
}

async fn run_tui(orchestrator: Arc<dyn OrchestratorApi>, model: String) {
    use crossterm::event::{KeyCode, KeyModifiers};
    use crossterm::execute;
    use crossterm::terminal::{
        EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    };
    use opca_cli::tui::app::App;
    use opca_cli::tui::event::{AppEvent, poll_event};
    use opca_cli::tui::input::InputArea;
    use opca_cli::tui::render::render;

    fn send_message(app: &mut App, msg: &str) {
        app.handle_message(msg);
        if matches!(
            app.chat_items.last(),
            Some(opca_cli::tui::app::ChatItem::StreamingAssistant(_))
        ) {
            let tx = app.stream_tx.clone();
            app.orchestrator.stream_foreground(msg, tx);
        }
    }
    use ratatui::Terminal;
    use ratatui::backend::CrosstermBackend;

    enable_raw_mode().expect("raw mode");
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).expect("alt screen");
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal.clear().ok();

    let mut app = App::new(orchestrator.clone(), model);
    let mut input = InputArea::new();
    let mut notif_rx = orchestrator.subscribe();

    loop {
        if app.is_working {
            app.spinner_frame = app.spinner_frame.wrapping_add(1);
        }
        terminal.draw(|f| render(f, &app, &input)).ok();

        match poll_event(&mut notif_rx) {
            Some(AppEvent::Key(key)) => match key.code {
                KeyCode::Enter => {
                    if !input.is_empty() {
                        let msg = input.input();
                        input.clear();
                        if app.is_working {
                            app.pending_messages.push_back(msg);
                        } else {
                            send_message(&mut app, &msg);
                        }
                    }
                }
                KeyCode::Esc => {
                    if app.is_working {
                        app.stop_working();
                    }
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.should_quit = true;
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.should_quit = true;
                }
                _ => {
                    input.textarea.input(key);
                }
            },
            Some(AppEvent::Notification(n)) => {
                app.handle_notification(&n);
            }
            Some(AppEvent::Tick) | None => {
                app.poll_stream();
                if !app.is_working {
                    if let Some(msg) = app.pending_messages.pop_front() {
                        send_message(&mut app, &msg);
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode().ok();
    execute!(std::io::stdout(), LeaveAlternateScreen).ok();
}

fn init_tracing(verbose: u8) {
    let default_filter = match verbose {
        0 => "opca=warn,warn",
        1 => "opca=info,info",
        2 => "opca=debug,debug",
        _ => "trace",
    };
    // RUST_LOG always wins if set.
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| default_filter.to_string());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

/// Construct a real LLM [`Provider`] from CLI flags, config, and environment.
///
/// Resolution order for the provider kind:
/// 1. `--provider` CLI flag
/// 2. `[provider] kind` in `.agent/config.toml`
/// 3. Inferred from the model name prefix (`claude`→anthropic, `glm`→zhipu, …)
/// 4. Fallback: `anthropic`
///
/// Returns `None` when no API key can be found for the resolved provider.
fn create_provider(
    args: &Cli,
    model: &str,
    config: &opca_core::config::Config,
) -> Option<Arc<dyn Provider>> {
    use opca_core::provider::presets::{self, ApiProtocol};
    use opca_core::provider::{AnthropicProvider, OpenAIProvider};

    let kind = args
        .provider_kind
        .as_deref()
        .or(config.provider.kind.as_deref())
        .or_else(|| guess_kind_from_model(model))
        .unwrap_or("anthropic");

    if let Some(preset) = presets::resolve(kind) {
        let api_key = if preset.env_key.is_empty() {
            String::from("unused")
        } else {
            std::env::var(preset.env_key).ok()?
        };
        let base_url = config
            .provider
            .base_url
            .as_deref()
            .unwrap_or(preset.base_url);
        return Some(match preset.api {
            ApiProtocol::OpenAIChat => {
                let url = presets::normalize_chat_completions_url(base_url);
                Arc::new(OpenAIProvider::with_base_url(&api_key, model, &url))
            }
            ApiProtocol::AnthropicMessages => Arc::new(AnthropicProvider::new(&api_key, model)),
        });
    }

    if let Some(base_url) = &config.provider.base_url {
        let api_key = std::env::var("OPCA_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .ok()?;
        let url = presets::normalize_chat_completions_url(base_url);
        return Some(Arc::new(OpenAIProvider::with_base_url(
            &api_key, model, &url,
        )));
    }

    None
}

/// Infer a provider kind from the model name prefix.
fn guess_kind_from_model(model: &str) -> Option<&'static str> {
    let lower = model.to_lowercase();
    if lower.starts_with("claude") {
        Some("anthropic")
    } else if lower.starts_with("gpt") {
        Some("openai")
    } else if lower.starts_with("gemini") {
        Some("gemini")
    } else if lower.starts_with("glm") {
        Some("zhipu")
    } else if lower.starts_with("deepseek") {
        Some("deepseek")
    } else {
        None
    }
}

/// Print the startup banner and the first-run onboarding hint.
///
/// First-run detection is intentionally local: it checks whether the
/// project's `.agent/sessions/` directory exists. If not, the user sees a
/// short orientation block listing the most useful slash commands. The
/// check is read-only; nothing is created here.
#[allow(dead_code)]
fn print_banner(args: &Cli, model: &str) {
    println!("opca — background-first code agent");
    println!("Type /help for commands, /quit to exit.");
    println!();

    let is_first_run = !args.project.join(SESSIONS_DIR).exists();
    if is_first_run {
        println!("Looks like this is a fresh workspace. Quick orientation:");
        println!("  - Describe a task in plain English and it goes to a background Task.");
        println!("          e.g.  refactor the auth module into its own crate");
        println!("  - /tasks                 list active Tasks");
        println!("  - /status <task-id>      inspect one Task");
        println!("  - /accept <task-id>      merge a delivered Task");
        println!("  - /reject <task-id> \"\"   send it back with feedback");
        println!("  - /help                  full command list");
        println!();
    }

    if args.verbose > 0 {
        println!(
            "[startup] model={} project={} session={}",
            args.project.display(),
            model,
            args.session.as_deref().unwrap_or("<new>")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_minimal_args() {
        let cli = Cli::try_parse_from(["opca", "--project", "."]).expect("parse");
        assert_eq!(cli.project, PathBuf::from("."));
        assert!(cli.session.is_none());
        assert!(cli.model.is_none());
        assert_eq!(cli.verbose, 0);
        assert!(!cli.mock);
    }

    #[test]
    fn cli_parses_all_args() {
        let cli = Cli::try_parse_from([
            "opca",
            "--project",
            "/tmp/demo",
            "--session",
            "abc-123",
            "--model",
            "claude-haiku",
            "-vv",
            "--mock",
        ])
        .expect("parse");
        assert_eq!(cli.project, PathBuf::from("/tmp/demo"));
        assert_eq!(cli.session.as_deref(), Some("abc-123"));
        assert_eq!(cli.model.as_deref(), Some("claude-haiku"));
        assert_eq!(cli.verbose, 2);
        assert!(cli.mock);
    }

    #[test]
    fn cli_verbose_counts_repeats() {
        let one = Cli::try_parse_from(["opca", "-v"]).expect("parse");
        let three = Cli::try_parse_from(["opca", "-vvv"]).expect("parse");
        assert_eq!(one.verbose, 1);
        assert_eq!(three.verbose, 3);
    }

    #[test]
    fn cli_rejects_unknown_flag() {
        let result = Cli::try_parse_from(["opca", "--bogus"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_parses_provider_flag() {
        let cli = Cli::try_parse_from(["opca", "--provider", "zhipu"]).expect("parse");
        assert_eq!(cli.provider_kind.as_deref(), Some("zhipu"));
    }

    #[test]
    fn cli_provider_flag_optional() {
        let cli = Cli::try_parse_from(["opca"]).expect("parse");
        assert!(cli.provider_kind.is_none());
    }

    #[test]
    fn guess_kind_from_model_prefixes() {
        assert_eq!(
            guess_kind_from_model("claude-sonnet-4-20250514"),
            Some("anthropic")
        );
        assert_eq!(guess_kind_from_model("Claude-3"), Some("anthropic"));
        assert_eq!(guess_kind_from_model("gpt-4o"), Some("openai"));
        assert_eq!(guess_kind_from_model("GPT-4"), Some("openai"));
        assert_eq!(guess_kind_from_model("gemini-1.5-pro"), Some("gemini"));
        assert_eq!(guess_kind_from_model("glm-5.2"), Some("zhipu"));
        assert_eq!(guess_kind_from_model("GLM-4-Flash"), Some("zhipu"));
        assert_eq!(guess_kind_from_model("deepseek-chat"), Some("deepseek"));
    }

    #[test]
    fn guess_kind_unknown_model_returns_none() {
        assert_eq!(guess_kind_from_model("llama-3"), None);
        assert_eq!(guess_kind_from_model("qwen-max"), None);
        assert_eq!(guess_kind_from_model(""), None);
    }
}
