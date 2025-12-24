use bincode::{Decode, Encode, config};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};
use uuid::Uuid;

#[derive(Encode, Decode, Debug)]
enum MessageType {
    Execution,
    Stdin,
}

#[derive(Encode, Decode, Debug)]
struct Message {
    message_type: MessageType,
    language: String,
    code: String,
}

#[derive(thiserror::Error, Debug)]
enum HandlerError {
    #[error("Failed to listen on port: {port}")]
    ListenerError { port: u16 },

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to decode: {0}")]
    DecodeError(#[from] bincode::error::DecodeError),
}

fn main() -> Result<(), HandlerError> {
    listen_to_port(8000)
}

fn listen_to_port(port: u16) -> Result<(), HandlerError> {
    let address = format!("0.0.0.0:{port}");
    let listener = TcpListener::bind(&address).map_err(|_| HandlerError::ListenerError { port })?;

    println!("Listening on {address}...");

    for stream_res in listener.incoming() {
        match stream_res {
            Ok(stream) => {
                handle_request(stream)?;
            }
            Err(e) => {
                eprintln!("Accept error: {}", e);
            }
        }
    }

    Ok(())
}

fn handle_request(mut stream: TcpStream) -> Result<(), HandlerError> {
    let message = read_content_from_stream(&mut stream)?;
    handle_message(message, stream)?;

    Ok(())
}

fn handle_message(message: Message, stream: TcpStream) -> Result<(), HandlerError> {
    let uuid = Uuid::new_v4();
    let work_dir = format!("/work/executions/{uuid}");
    fs::create_dir_all(&work_dir)?;

    let script_path = format!("{work_dir}/main.rs");
    fs::write(&script_path, message.code)?;
    let executable_path = format!("{}/main", &work_dir);

    let mut child = Command::new("rustc")
        .args([&script_path, "-o", &executable_path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let shared_stream = Arc::new(Mutex::new(stream));

    let (out_handle, _err_handle) = get_out_err_handlers(&shared_stream, &mut child);

    let status = child.wait()?;
    let _ = out_handle.join();
    // let _ = err_handle.join();

    if !status.success() {
        return Ok(());
    }

    let mut child = Command::new(executable_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let (out_handle, err_handle) = get_out_err_handlers(&shared_stream, &mut child);

    let status = child.wait()?;
    let _ = out_handle.join();
    let _ = err_handle.join();

    writeln!(
        shared_stream.lock().unwrap(),
        "Process exited with status: {status}"
    )?;

    fs::remove_dir_all(&work_dir)?;

    clean_up()
}

fn read_content_from_stream(stream: &mut TcpStream) -> Result<Message, HandlerError> {
    let mut content_length_buffer = [0u8; 4];
    stream.read_exact(&mut content_length_buffer)?;

    let content_length = u32::from_ne_bytes(content_length_buffer);

    let mut message_buffer = vec![0u8; content_length as usize];
    stream.read_exact(&mut message_buffer)?;

    let (message, _message_length): (Message, usize) =
        bincode::decode_from_slice(&message_buffer[..], config::standard())?;
    Ok(message)
}

fn get_out_err_handlers(
    shared_stream: &Arc<Mutex<TcpStream>>,
    child: &mut Child,
) -> (JoinHandle<()>, JoinHandle<()>) {
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // stdout thread
    let out_stream = Arc::clone(&shared_stream);
    let out_handle = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if let Ok(l) = line {
                let mut s = out_stream.lock().unwrap();
                let _ = writeln!(s, "{l}");
            }
        }
    });

    // stderr thread
    let err_stream = Arc::clone(&shared_stream);
    let err_handle = thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if let Ok(l) = line {
                let mut s = err_stream.lock().unwrap();
                let _ = writeln!(s, "{l}");
            }
        }
    });

    (out_handle, err_handle)
}

fn clean_up() -> Result<(), HandlerError> {
    let _ = Command::new("find")
        .args(["/work", "-mindepth", "1", "-delete"])
        .status()?;

    let _ = Command::new("find")
        .args(["/tmp", "-mindepth", "1", "-delete"])
        .status()?;

    Ok(())
}
