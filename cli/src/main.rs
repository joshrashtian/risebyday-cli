mod config;
mod session;
mod supabase;

use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueHint::Unknown};
use colored::*;
use config::Config;
use session::Session;
use std::io::{self, Write};
use supabase::{Supabase, TaskInsert, TaskRow};

#[derive(Parser, Debug)]
#[command(version, about = "What About", long_about = "Lets have some fun")]
pub struct CLI {
    #[clap(subcommand)]
    pub command: CommandType,
}

#[derive(Subcommand, Debug)]
pub enum CommandType {
    /// Interface with tasks
    Task(TaskCommand),
    /// Sign in to your RiseByDay account
    SignIn(SignInCommand),
    /// Forget the saved session
    SignOut,
    /// Show who is signed in
    Whoami,
}

#[derive(Debug, Args)]
pub struct TaskCommand {
    #[clap(subcommand)]
    pub command: TaskSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum TaskSubcommand {
    /// Create New Task
    Create(CreateTask),
    /// List open tasks
    List(ListTasks),
}

#[derive(Debug, Args)]
pub struct ListTasks {
    /// Option for list, such as all, uncompleted or completed. Defaults to uncompleted.
    #[arg(short, long)]
    pub filter: Option<String>,
}

#[derive(Debug, Args)]
pub struct CreateTask {
    /// Name of the task
    #[arg(short, long)]
    pub name: Option<String>,
    /// Due date/time, ISO 8601 (e.g. 2026-09-18T17:00:00Z)
    #[arg(short, long)]
    pub due: Option<String>,
    /// low | medium | high
    #[arg(short, long)]
    pub priority: Option<String>,
    /// Block name, as configured in the app
    #[arg(short, long)]
    pub block: Option<String>,
    /// Category name, as configured in the app
    #[arg(short, long)]
    pub category: Option<String>,
}

#[derive(Debug, Args)]
pub struct SignInCommand {
    /// Your email to sign in
    #[arg(short, long)]
    pub email: String,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{} {err:#}", "error:".red().bold());
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = CLI::parse();
    let supabase = Supabase::new(Config::load()?)?;

    match cli.command {
        CommandType::Task(task_cmd) => match task_cmd.command {
            TaskSubcommand::Create(args) => create_task(&supabase, args).await,
            TaskSubcommand::List(args) => list_tasks(&supabase, args).await,
        },
        CommandType::SignIn(args) => sign_in(&supabase, args.email).await,
        CommandType::SignOut => sign_out(&supabase).await,
        CommandType::Whoami => whoami(),
    }
}

async fn create_task(supabase: &Supabase, args: CreateTask) -> Result<()> {
    let session = supabase.authed().await?;

    let name = match args.name {
        Some(n) if !n.trim().is_empty() => n,
        _ => prompt("What is your task name?")?,
    };

    let task = TaskInsert {
        title: name,
        due_date: args.due,
        priority: args.priority,
        block: args.block,
        category: args.category,
    };
    let row = supabase.create_task(&session, &task).await?;

    println!("{} {}", "Created:".green().bold(), row.title);
    println!("  {}", row.id.dimmed());
    Ok(())
}

async fn list_tasks(supabase: &Supabase, args: ListTasks) -> Result<()> {
    let session = supabase.authed().await?;
    let rows = match args.filter.as_deref() {
        Some("all") => supabase.list_all_tasks(&session).await?,
        Some("open") | _ => supabase.list_open_tasks(&session).await?,
    };

    if rows.is_empty() {
        println!("{}", "No open tasks.".dimmed());
        return Ok(());
    }
    for row in &rows {
        print_task(row);
    }
    println!("{}", format!("{} open", rows.len()).dimmed());
    Ok(())
}

fn print_task(row: &TaskRow) {
    let due = row
        .due_date
        .as_deref()
        .map(|d| d.chars().take(16).collect::<String>().replace('T', " "))
        .unwrap_or_else(|| "no due date".to_string());
    let priority = match row.priority.as_deref() {
        Some("high") => "!!!".red().bold(),
        Some("medium") => "!! ".yellow(),
        Some("low") => "!  ".dimmed(),
        _ => "   ".normal(),
    };
    let mut tags = Vec::new();
    if let Some(block) = &row.block {
        tags.push(format!("@{block}"));
    }
    if let Some(category) = &row.category {
        tags.push(format!("#{category}"));
    }
    println!(
        "{priority} {:<40} {}  {}",
        row.title,
        due.dimmed(),
        tags.join(" ").cyan()
    );
}

async fn sign_in(supabase: &Supabase, email: String) -> Result<()> {
    println!("{} {}", "Signing in as:".yellow().bold(), email);
    let password = rpassword::prompt_password("Password: ")?;

    let session = supabase.sign_in(email.trim(), &password).await?;
    session.save()?;

    println!(
        "{} {}",
        "Signed in.".green().bold(),
        session.email.as_deref().unwrap_or(&session.user_id)
    );
    Ok(())
}

async fn sign_out(supabase: &Supabase) -> Result<()> {
    if let Some(session) = Session::load()? {
        supabase.sign_out(&session).await?;
    }
    Session::clear()?;
    println!("{}", "Signed out.".green().bold());
    Ok(())
}

fn whoami() -> Result<()> {
    match Session::load()? {
        Some(session) => {
            println!(
                "{} {}",
                "Signed in as:".green().bold(),
                session.email.as_deref().unwrap_or("(no email)")
            );
            println!("  {}", session.user_id.dimmed());
        }
        None => println!("{}", "Not signed in.".dimmed()),
    }
    Ok(())
}

fn prompt(question: &str) -> Result<String> {
    println!("{}", question.yellow().bold());
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}
