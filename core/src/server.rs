//! The server. One machine runs it; everyone else dials in.
use crate::config;
use crate::history;
use crate::msg::{self, Msg, ACTOR_AI, KIND_CHANGE, KIND_CHAT};
use crate::wire::{Conn, Welcome};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};

struct Sub {
    id: u64,
    channel: String,
    tx: SyncSender<Msg>,
}

pub struct Hub {
    subs: Mutex<Vec<Sub>>,
    seq: AtomicI64,
    next_id: AtomicU64,
}

impl Hub {
    fn new(seq: i64) -> Arc<Hub> {
        Arc::new(Hub {
            subs: Mutex::new(Vec::new()),
            seq: AtomicI64::new(seq),
            next_id: AtomicU64::new(1),
        })
    }

    fn subscribe(&self, channel: &str) -> (u64, Receiver<Msg>) {
        let (tx, rx) = sync_channel(256);
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.subs.lock().unwrap().push(Sub {
            id,
            channel: channel.to_string(),
            tx,
        });
        (id, rx)
    }

    fn unsubscribe(&self, id: u64) {
        self.subs.lock().unwrap().retain(|s| s.id != id);
    }

    fn publish(&self, mut m: Msg) {
        m.seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        history::append(&m);
        {
            let subs = self.subs.lock().unwrap();
            for s in subs.iter() {
                if !s.channel.is_empty() && s.channel != m.channel {
                    continue;
                }
                // A stalled reader must not block everyone else.
                let _ = s.tx.try_send(m.clone());
            }
        }
        println!("[{}] #{} {}: {}", m.channel, m.seq, m.label(), m.line());
    }
}

pub fn serve() -> ! {
    // Resume numbering across restarts.
    let resume = history::read().iter().map(|m| m.seq).max().unwrap_or(0);
    let hub = Hub::new(resume);

    let port = config::port();
    let listener = match TcpListener::bind(format!("0.0.0.0:{port}")) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("collab: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "collab server on {} (port {}), resuming at #{resume}",
        config::hostname(),
        port
    );
    println!(
        "others use:  host = {}  in ~/.collab-config",
        config::hostname()
    );

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let hub = Arc::clone(&hub);
        std::thread::spawn(move || handle(hub, stream));
    }
    unreachable!()
}

fn handle(hub: Arc<Hub>, stream: TcpStream) {
    let _ = stream.set_nodelay(true);
    // A frame that will not open is not a message. Someone on the network with
    // the wrong key, or no key, gets nothing and tells us nothing.
    // Which channel this is comes from which key opened the frame, not from
    // anything the client claims — so a client cannot reach a channel it does
    // not hold the key for by simply naming it.
    let (mut conn, channel, mut hello) = match Conn::accept(stream) {
        Ok(x) => x,
        Err(_) => return,
    };
    hello.channel = channel;
    // The Hello opened, so both sides hold the same key. Say so — otherwise a
    // client with the wrong word cannot tell refusal from delivery.
    if conn
        .send(&Welcome {
            ok: true,
            server: config::hostname(),
        })
        .is_err()
    {
        return;
    }

    match hello.mode.as_str() {
        // History in one shot, then hang up — for `collab log` and the read
        // tools on the machine that is not the server, whose own copy is empty.
        "fetch" => {
            for m in history::filter(history::read(), &hello.channel, hello.since) {
                if conn.send(&m).is_err() {
                    return;
                }
            }
        }

        // Everything missed, then live. No gap, by construction.
        "watch" => {
            for m in history::filter(history::read(), &hello.channel, hello.since) {
                if conn.send(&m).is_err() {
                    return;
                }
            }
            let (id, rx) = hub.subscribe(&hello.channel);
            while let Ok(m) = rx.recv() {
                if conn.send(&m).is_err() {
                    break;
                }
            }
            hub.unsubscribe(id);
        }

        // post: every remaining frame is a message.
        _ => {
            while let Ok(Some(mut m)) = conn.recv::<Msg>() {
                if m.text.trim().is_empty() && m.target.is_empty() {
                    continue;
                }
                // The connection says who and where; the payload does not get a vote.
                m.seq = 0;
                m.channel = hello.channel.clone();
                m.from = hello.name.clone();
                m.host = hello.host.clone();
                m.at = msg::now();
                if m.kind != KIND_CHANGE {
                    m.kind = KIND_CHAT.into();
                    m.action.clear();
                    m.target.clear();
                }
                if m.via != ACTOR_AI {
                    m.via.clear();
                }
                hub.publish(m);
            }
        }
    }
}
