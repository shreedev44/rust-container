use bincode::{Decode, Encode, config};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
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

    // #[error("Thread join error: {0}")]
    // ThreadJoinError(String),
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
    let work_dir = format!("sandbox/tmp/executions/{uuid}");
    fs::create_dir_all(&work_dir)?;

    let script_path = format!("{work_dir}/script.js");
    fs::write(&script_path, message.code)?;

    let mut child = Command::new("node")
    .arg(&script_path)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;


    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let shared_stream = Arc::new(Mutex::new(stream));

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

    let status = child.wait()?;
    let _ = out_handle.join();
    let _ = err_handle.join();

    writeln!(
        shared_stream.lock().unwrap(),
        "Process exited with status: {status}"
    )?;

    fs::remove_dir_all(&work_dir)?;

    Ok(())
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
