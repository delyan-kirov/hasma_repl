use libc::{tcgetattr, tcsetattr, ECHO, ICANON, TCSANOW};
use std::io::{self, Read, Write};
use std::mem;
use std::os::unix::io::{AsRawFd, RawFd};

#[non_exhaustive]
struct Key {}

impl Key {
    pub const BACKSPACE: u8 = b'\x7F'; // ASCII 127
    pub const ENTER: u8 = b'\n'; // ASCII 10
    pub const CTRL_D: u8 = 4; // ASCII 4
    pub const ESCAPE: u8 = 27; // ESC key (ASCII 27)
    pub const ARROW_UP: (u8, u8) = (b'[', b'A'); // Arrow Up (ESC [ A)
    pub const ARROW_DOWN: (u8, u8) = (b'[', b'B'); // Arrow Down (ESC [ B)
    pub const ARROW_RIGHT: (u8, u8) = (b'[', b'C'); // Arrow Right (ESC [ C)
    pub const ARROW_LEFT: (u8, u8) = (b'[', b'D'); // Arrow Left (ESC [ D)
}

fn set_raw_mode(fd: RawFd) -> io::Result<()> {
    let mut termios = unsafe {
        let mut termios = mem::zeroed();
        if tcgetattr(fd, &mut termios) != 0 {
            return Err(io::Error::last_os_error());
        }
        termios
    };

    termios.c_lflag &= !(ICANON | ECHO); // Disable canonical mode and echo
    termios.c_cc[libc::VMIN] = 1; // Minimum number of characters for read()
    termios.c_cc[libc::VTIME] = 0; // No timeout

    if unsafe { tcsetattr(fd, TCSANOW, &termios) != 0 } {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

fn restore_mode(fd: RawFd) -> io::Result<()> {
    let mut termios = unsafe {
        let mut termios = mem::zeroed();
        if tcgetattr(fd, &mut termios) != 0 {
            return Err(io::Error::last_os_error());
        }
        termios
    };

    termios.c_lflag |= ICANON | ECHO; // Restore canonical mode and echo

    if unsafe { tcsetattr(fd, TCSANOW, &termios) != 0 } {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

struct Canvas<'a> {
    handle: io::StdoutLock<'a>,
    buffer: Vec<Vec<u8>>,
    x: usize,
    y: usize,
}
impl<'a> Write for Canvas<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.handle.write(buf) {
            Ok(n) => {
                self.flush()?;
                Ok(n)
            }
            Err(n) => Err(n),
        }
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.handle.write_all(buf)?;
        self.handle.flush()
    }

    fn flush(&mut self) -> io::Result<()> {
        self.handle.flush()
    }
}
impl<'a> Canvas<'a> {
    fn new() -> Self {
        let stdout = io::stdout();
        Canvas {
            handle: stdout.lock(),
            x: 0,
            y: 0,
            buffer: vec![vec![]],
        }
    }
    fn clear_view(&mut self) -> io::Result<()> {
        self.write_all(b"\x1B[2J\x1B[H")
    }
    fn render(&mut self) -> io::Result<()> {
        self.clear_view()?;
        for (i, line) in self.buffer.clone().iter().enumerate() {
            self.write_all(
                format!("\x1B[{};1H{}", i + 1, String::from_utf8_lossy(line)).as_bytes(),
            )?;
        }
        self.write_all(format!("\x1B[{};{}H", self.y + 1, self.x + 1).as_bytes())
    }
    fn jump_to_top(&mut self) -> io::Result<()> {
        self.x = 0;
        self.y = 0;
        self.render()?;
        Ok(())
    }
    fn move_up(&mut self) {
        if self.y > 0 {
            self.y -= 1;
            self.x = 0; // need to set it to something
        }
    }

    fn move_down(&mut self) {
        if self.y < self.buffer.len() - 1 {
            self.y += 1;
            self.x = 0; // need to set it to something
        }
    }

    fn move_right(&mut self) {
        if self.x < self.buffer[self.y].len() {
            self.x += 1;
        }
    }

    fn move_left(&mut self) {
        if self.x > 0 {
            self.x -= 1;
        }
    }
}

fn main() -> io::Result<()> {
    let mut stdin = io::stdin();
    let stdin_fd = stdin.lock().as_raw_fd();

    let mut canvas = Canvas::new();
    canvas.clear_view()?;

    // Set terminal to raw mode
    set_raw_mode(stdin_fd)?;
    canvas.clear_view()?;

    let mut input_buffer = [0; 4]; // Buffer to handle escape sequences
    let mut index = 0;

    loop {
        let mut buf = [0; 1];
        match stdin.read(&mut buf) {
            Ok(0) => {
                // No input available, continue the loop
            }
            Ok(_) => {
                input_buffer[index] = buf[0];
                index += 1;

                // Handle escape sequences
                if input_buffer[0] == Key::ESCAPE {
                    if index >= 3 {
                        // Handle arrow keys (ESC [ A/B/C/D)
                        match (input_buffer[1], input_buffer[2]) {
                            Key::ARROW_UP => {
                                canvas.move_up(); // Move up
                            }
                            Key::ARROW_DOWN => {
                                canvas.move_down(); // Move down
                            }
                            Key::ARROW_RIGHT => {
                                canvas.move_right(); // Move right
                            }
                            Key::ARROW_LEFT => {
                                canvas.move_left();
                            }
                            _ => (),
                        }
                        index = 0;
                    }
                } else {
                    // Handle regular characters
                    match buf[0] {
                        Key::CTRL_D => {
                            break;
                        }
                        Key::ENTER => {
                            if canvas.y < canvas.buffer.len() {
                                canvas.buffer.insert(canvas.y + 1, Vec::new());
                            } else {
                                // If we're at the last line, just add the new line at the end
                                canvas.buffer.push(Vec::new());
                            }
                            if canvas.x == canvas.buffer[canvas.y].len() {
                                canvas.move_down(); // Move to next line
                            } else {
                                let mut left_over = canvas.buffer[canvas.y]
                                    .drain(canvas.x..)
                                    .collect::<Vec<u8>>();
                                canvas.move_down(); // Move to next line
                                canvas.buffer[canvas.y].append(&mut left_over);
                            }
                        }
                        Key::BACKSPACE => {
                            if canvas.x > 0 {
                                // If the canvas is not at the beginning of the line, just remove the character
                                canvas.buffer[canvas.y].remove(canvas.x - 1);
                                canvas.x -= 1;
                            } else if canvas.y > 0 {
                                // Move the canvas to the previous line
                                canvas.y -= 1;
                                // Remove the current line and get its content
                                let mut left_over: Vec<u8> = canvas.buffer.remove(canvas.y + 1); // Get the next line
                                let new_x = canvas.buffer[canvas.y].len();
                                // Append the current line to the previous one
                                canvas.buffer[canvas.y].append(&mut left_over);
                                // Adjust the canvas position: it should remain where it was in the previous line
                                canvas.x = new_x;
                            } else {
                                // At the very beginning of the buffer, do nothing
                            }
                        }
                        c => {
                            if canvas.x == canvas.buffer[canvas.y].len() {
                                canvas.buffer[canvas.y].push(c);
                            } else {
                                canvas.buffer[canvas.y].insert(canvas.x, c);
                            }
                            canvas.x += 1;
                        }
                    }
                    index = 0;
                }
            }
            Err(_) => {
                break;
            }
        }
        canvas.render()?;
    }

    // Restore terminal settings and show canvas
    canvas.jump_to_top()?;
    restore_mode(stdin_fd)?;
    canvas.clear_view()?;

    Ok(())
}
