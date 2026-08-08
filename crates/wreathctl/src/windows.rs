use std::env;

use wreath_core::config::{Codec, Config, HotkeyConfig};
use wreath_core::ipc::{Request, Response};
use wreath_core::paths::AppPaths;

pub fn run() -> Result<(), String> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let command = arguments.first().map(String::as_str).unwrap_or("status");
    match command {
        "monitors" => return print_monitors(),
        "microphones" => return print_microphones(),
        "codecs" => return print_hardware_codecs(),
        "config" => return configure(&arguments[1..]),
        "cut" => return crate::cut::run(&arguments[1..]),
        _ => {}
    }
    let request = match command {
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

fn print_hardware_codecs() -> Result<(), String> {
    let runtime =
        wreath_windows::video::VideoRuntime::initialize().map_err(|error| error.to_string())?;
    let support = runtime.support();
    for (name, available) in [
        ("h264", support.h264),
        ("hevc", support.hevc),
        ("av1", support.av1),
    ] {
        if available {
            println!("{name}");
        }
    }
    Ok(())
}

fn print_monitors() -> Result<(), String> {
    for display in wreath_windows::display::displays().map_err(|error| error.to_string())? {
        println!(
            "{}  {}x{} @ {:.0} Hz{}",
            display.name,
            display.width,
            display.height,
            display.refresh_rate,
            if display.primary { " [primary]" } else { "" }
        );
    }
    Ok(())
}

fn print_microphones() -> Result<(), String> {
    for microphone in wreath_windows::audio::microphones().map_err(|error| error.to_string())? {
        println!(
            "{}  {}{}",
            microphone.name,
            microphone.id,
            if microphone.default { " [default]" } else { "" }
        );
    }
    Ok(())
}

fn configure(arguments: &[String]) -> Result<(), String> {
    let paths = AppPaths::discover();
    let mut config = Config::load(&paths).map_err(|error| error.to_string())?;
    let Some(setting) = arguments.first().map(String::as_str) else {
        let encoded = toml::to_string_pretty(&config).map_err(|error| error.to_string())?;
        print!("{encoded}");
        return Ok(());
    };
    let previous = config.clone();
    let value = arguments
        .get(1)
        .ok_or_else(|| format!("missing value for `{setting}`"))?;
    apply_setting(&mut config, setting, value)?;
    config.save(&paths).map_err(|error| error.to_string())?;
    match wreath_windows::control::send_request(paths.pipe_name(), &Request::Reload) {
        Ok(Response::Ok) => {}
        Ok(Response::Error { message }) => {
            previous.save(&paths).map_err(|restore_error| {
                format!(
                    "reload rejected the change ({message}); previous settings could not be restored: {restore_error}"
                )
            })?;
            let _ = wreath_windows::control::send_request(paths.pipe_name(), &Request::Reload);
            return Err(format!(
                "reload rejected the change ({message}); previous settings restored"
            ));
        }
        Ok(_) => {
            previous.save(&paths).map_err(|restore_error| {
                format!("unexpected reload response; previous settings could not be restored: {restore_error}")
            })?;
            let _ = wreath_windows::control::send_request(paths.pipe_name(), &Request::Reload);
            return Err("unexpected reload response; previous settings restored".into());
        }
        Err(error) => eprintln!("settings saved; recorder is not running ({error})"),
    }
    println!("saved {setting} in {}", paths.config_file.display());
    Ok(())
}

fn apply_setting(config: &mut Config, setting: &str, value: &str) -> Result<(), String> {
    match setting {
        "monitor" if value.eq_ignore_ascii_case("default") => config.capture.monitor = None,
        "monitor" => {
            let displays =
                wreath_windows::display::displays().map_err(|error| error.to_string())?;
            let display = displays
                .iter()
                .find(|display| display.name.eq_ignore_ascii_case(value))
                .ok_or_else(|| format!("unknown monitor `{value}`; run `wreathctl monitors`"))?;
            config.capture.monitor = Some(display.name.clone());
        }
        "microphone" if value.eq_ignore_ascii_case("off") => {
            config.audio.microphone = false;
            config.audio.microphone_device = None;
        }
        "microphone" if value.eq_ignore_ascii_case("default") => {
            config.audio.microphone = true;
            config.audio.microphone_device = None;
        }
        "microphone" => {
            let microphones =
                wreath_windows::audio::microphones().map_err(|error| error.to_string())?;
            let microphone = microphones
                .iter()
                .find(|microphone| microphone.id == value)
                .ok_or_else(|| {
                    "unknown microphone endpoint; run `wreathctl microphones`".to_owned()
                })?;
            config.audio.microphone = true;
            config.audio.microphone_device = Some(microphone.id.clone());
        }
        "desktop-audio" => config.audio.desktop = parse_switch(value, setting)?,
        "microphone-gain" => config.audio.microphone_gain_percent = parse_number(value, setting)?,
        "hotkey" => {
            let hotkey = HotkeyConfig::parse(value).map_err(|error| error.to_string())?;
            wreath_windows::hotkey::NativeHotkey::try_from(&hotkey)
                .map_err(|error| error.to_string())?;
            config.hotkey = hotkey;
        }
        "duration" => config.capture.duration_seconds = parse_number(value, setting)?,
        "fps" => config.capture.frames_per_second = parse_number(value, setting)?,
        "quality" => config.capture.quality = parse_number(value, setting)?,
        "codec" => {
            config.capture.codec = match value.to_ascii_lowercase().as_str() {
                "auto" => Codec::Auto,
                "h264" => Codec::H264,
                "hevc" => Codec::Hevc,
                "av1" => Codec::Av1,
                _ => return Err("codec must be auto, h264, hevc, or av1".into()),
            }
        }
        "cursor" => config.capture.cursor = parse_switch(value, setting)?,
        "output" => config.storage.directory = value.into(),
        _ => {
            return Err(format!(
                "unknown setting `{setting}`; use monitor, microphone, desktop-audio, microphone-gain, hotkey, duration, fps, quality, codec, cursor, or output"
            ));
        }
    }
    config.validate().map_err(|error| error.to_string())
}

fn parse_number<T>(value: &str, name: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| format!("{name} must be a number"))
}

fn parse_switch(value: &str, name: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Ok(true),
        "off" | "false" | "no" | "0" => Ok(false),
        _ => Err(format!("{name} must be on or off")),
    }
}

fn print_response(response: Response) -> Result<(), String> {
    match response {
        Response::Status {
            state,
            monitor,
            codec,
            adapter,
            replay_bytes,
            buffered_seconds,
            error,
        } => {
            println!("state    {state:?}");
            println!("monitor  {}", monitor.as_deref().unwrap_or("none"));
            println!("codec    {}", codec.as_deref().unwrap_or("none"));
            if let Some(adapter) = adapter {
                println!(
                    "adapter  {:04x}:{:04x} {}",
                    adapter.vendor_id, adapter.device_id, adapter.name
                );
            }
            if let Some(replay_bytes) = replay_bytes {
                println!("replay   {replay_bytes} bytes");
            }
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
         commands:\n  monitors     list active displays\n  microphones  list active microphone endpoint IDs\n  \
         codecs       list available hardware video encoders\n  config       show or change local settings\n  \
         cut          cut a saved clip down to one span\n  \
         status       show daemon state\n  save         save the replay buffer\n  \
         pause     pause capture\n  resume    resume capture\n  reload    reload configuration\n  \
         shutdown  stop the daemon\n\n\
         examples:\n  wreathctl config monitor \\\\.\\DISPLAY1\n  \
         wreathctl config microphone default\n  wreathctl config microphone off\n  \
         wreathctl config duration 30\n  wreathctl config fps 60\n  \
         wreathctl config codec h264\n  \
         wreathctl cut C:\\Videos\\Wreath\\clip.mp4 8 20 --name \"Best bit\""
    );
}
