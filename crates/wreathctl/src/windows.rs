use std::env;

use wreath_core::ipc::{Request, Response};
use wreath_core::paths::AppPaths;

pub fn run() -> Result<(), String> {
    let command = env::args().nth(1).unwrap_or_else(|| "status".into());
    let request = match command.as_str() {
        "status" => Request::Status,
        "save" => Request::Save,
        "pause" => Request::Pause,
        "resume" => Request::Resume,
        "reload" => Request::Reload,
        "shutdown" => Request::Shutdown,
        "help" | "--help" | "-h" => {
            print_help();
            return Ok(());
        }
        _ => return Err(format!("unknown command `{command}`; use `wreathctl help`")),
    };
    let paths = AppPaths::discover();
    let response = wreath_windows::control::send_request(paths.pipe_name(), &request)
        .map_err(|error| error.to_string())?;
    print_response(response)
}

fn print_response(response: Response) -> Result<(), String> {
    match response {
        Response::Status {
            state,
            monitor,
            buffered_seconds,
            error,
        } => {
            println!("state    {state:?}");
            println!("monitor  {}", monitor.as_deref().unwrap_or("none"));
            println!("buffer   {buffered_seconds}s");
            if let Some(error) = error {
                println!("error    {error}");
            }
            Ok(())
        }
        Response::Saved { path } => {
            println!("saved {}", path.display());
            Ok(())
        }
        Response::Ok => Ok(()),
        Response::Error { message } => Err(message),
    }
}

fn print_help() {
    println!(
        "wreathctl <command>\n\n\
         commands:\n  status    show daemon state\n  save      save the replay buffer\n  \
         pause     pause capture\n  resume    resume capture\n  reload    reload configuration\n  \
         shutdown  stop the daemon"
    );
}
