use bincode::{Decode, Encode, config};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self},
};
use uuid::Uuid;

#[derive(Encode, Decode, Debug)]
struct ExecRequest {
    code: Vec<u8>,
    // args: Vec<String>,
    // env: Vec<(String, String)>,
    timeout_ms: u64,
}


#[derive(Clone, Copy)]
enum FrameType {
    ExecRequest = 1,
    Stdin       = 2,
    Stdout      = 3,
    Stderr      = 4,
    Exit        = 5,
    Error       = 6,
}

impl FrameType {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::ExecRequest),
            2 => Some(Self::Stdin),
            3 => Some(Self::Stdout),
            4 => Some(Self::Stderr),
            5 => Some(Self::Exit),
            6 => Some(Self::Error),
            _ => None,
        }
    }
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
    if let Some((_, payload)) = read_frame(&mut stream)? {
        let (message, _) = bincode::decode_from_slice(&payload, config::standard())?;
        handle_message(message, stream)?;
    }

    Ok(())
}

fn handle_message(request: ExecRequest, mut stream: TcpStream) -> Result<(), HandlerError> {
    let uuid = Uuid::new_v4();
    let work_dir = format!("/work/executions/{uuid}");
    fs::create_dir_all(&work_dir)?;

    let script_path = format!("{work_dir}/main.rs");
    fs::write(&script_path, request.code)?;
    let executable_path = format!("{}/main", &work_dir);

    let mut child = Command::new("rustc")
        .args([&script_path, "-o", &executable_path])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let out_stream = stream.try_clone()?;
    let err_stream = stream.try_clone()?;

    stream_pipe(child.stdout.take().unwrap(), out_stream, FrameType::Stdout);
    stream_pipe(child.stderr.take().unwrap(), err_stream, FrameType::Stderr);

    let status = child.wait()?;

    if !status.success() {
        return Ok(());
    }

    let mut child = Command::new(executable_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let out_stream = stream.try_clone()?;
    let err_stream = stream.try_clone()?;
    let in_stream = stream.try_clone()?;

    stream_pipe(child.stdout.take().unwrap(), out_stream, FrameType::Stdout);
    stream_pipe(child.stderr.take().unwrap(), err_stream, FrameType::Stderr);

    let stdin = child.stdin.take().unwrap();

    let child_done = Arc::new(AtomicBool::new(false));
    let done_flag = child_done.clone();

    thread::spawn(move || {
        handle_stdin(stdin, in_stream, done_flag);
    });

    let status = child.wait()?;

    child_done.store(true, Ordering::Relaxed);


    let code = status.code().unwrap_or(-1);
    send_frame(&mut stream, FrameType::Exit, &code.to_be_bytes())?;
    fs::remove_dir_all(&work_dir)?;
    clean_up()
}

fn handle_stdin(
    mut child_stdin: impl Write,
    mut stream: TcpStream,
    child_done: Arc<AtomicBool>,
) {
    while !child_done.load(Ordering::Acquire) {
        match read_frame(&mut stream) {
            Ok(Some((FrameType::Stdin, payload))) => {
                if child_stdin.write_all(&payload).is_err() {
                    break;
                }
                let _ = child_stdin.flush();
            }

            Ok(Some((_other, _))) => {
                continue;
            }

            Ok(None) => {
                break;
            }

            Err(_) => break,
        }
    }

    drop(child_stdin);
}

fn stream_pipe(
    mut reader: impl Read + Send + 'static,
    mut stream: TcpStream,
    frame_type: FrameType,
) {
    thread::spawn(move || {
        let mut buf = [0u8; 4096];

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if send_frame(&mut stream, frame_type, &buf[..n]).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
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


fn send_frame(
    stream: &mut TcpStream,
    typ: FrameType,
    payload: &[u8],
) -> std::io::Result<()> {
    stream.write_all(&[typ as u8])?;
    stream.write_all(&(payload.len() as u32).to_be_bytes())?;
    stream.write_all(payload)?;
    stream.flush()
}


fn read_frame(stream: &mut TcpStream) -> std::io::Result<Option<(FrameType, Vec<u8>)>> {
    let mut header = [0u8; 5];

    if let Err(e) = stream.read_exact(&mut header) {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            return Ok(None);
        }
        return Err(e);
    }

    let len = u32::from_be_bytes(header[1..5].try_into().unwrap());
    let mut payload = vec![0; len as usize];
    stream.read_exact(&mut payload)?;

    Ok(Some((FrameType::from_u8(header[0]).unwrap(), payload)))
}