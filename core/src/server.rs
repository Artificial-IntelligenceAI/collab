//! The server. One machine runs it; everyone else dials in.
use crate::config;
use crate::history;
use crate::msg::{self, Msg, ACTOR_AI, KIND_CHANGE, KIND_CHAT};
use crate::files;
use crate::wire::{Ack, Conn, FileHeader, Want, Welcome};
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
    let resume = history::read()
        .iter()
        .map(|m| m.seq)
        .max()
        .unwrap_or(0)
        .max(history::seq_floor());
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
    let creator = crate::channels::get(&hello.channel)
        .map(|c| c.creator_name())
        .unwrap_or_default();
    if conn
        .send(&Welcome {
            ok: true,
            server: config::hostname(),
            creator,
        })
        .is_err()
    {
        return;
    }

    match hello.mode.as_str() {
        // Taking a file in. The bytes are checked against the hash the sender
        // announced before anything is stored or published — a store that
        // accepts whatever it is handed is not content-addressed, it is just a
        // directory with confusing names.
        "put" => {
            let Ok(Some(hdr)) = conn.recv::<FileHeader>() else { return };
            if hdr.file.size > files::MAX_BYTES {
                let _ = conn.send(&Ack {
                    ok: false,
                    detail: format!("too big — the limit is {}", files::human(files::MAX_BYTES)),
                });
                return;
            }
            let mut data: Vec<u8> = Vec::new();
            loop {
                match conn.recv_raw() {
                    Ok(Some(chunk)) if chunk.is_empty() => break,
                    Ok(Some(chunk)) => {
                        data.extend_from_slice(&chunk);
                        if data.len() as u64 > files::MAX_BYTES {
                            let _ = conn.send(&Ack { ok: false, detail: "too big".into() });
                            return;
                        }
                    }
                    _ => return, // hung up mid-file: nothing is stored, nothing published
                }
            }
            let hash = files::hash_bytes(&data);
            if data.len() as u64 != hdr.file.size || hash != hdr.file.hash {
                let _ = conn.send(&Ack {
                    ok: false,
                    detail: "the bytes did not match what was announced — nothing stored".into(),
                });
                return;
            }
            if files::save_blob(&hello.channel, &hash, &data).is_err() {
                let _ = conn.send(&Ack { ok: false, detail: "could not store it".into() });
                return;
            }
            let name = files::safe_component(&hdr.file.name);
            hub.publish(Msg {
                channel: hello.channel.clone(),
                from: hello.name.clone(),
                host: hello.host.clone(),
                at: msg::now(),
                kind: crate::msg::KIND_FILE.into(),
                via: if hdr.via == ACTOR_AI { ACTOR_AI.into() } else { String::new() },
                text: hdr.caption.replace('\n', " "),
                file: Some(files::FileRef { name: name.clone(), size: hdr.file.size, hash: hash.clone() }),
                ..Default::default()
            });
            let _ = conn.send(&Ack {
                ok: true,
                detail: format!("{name} ({}) sent", files::human(hdr.file.size)),
            });
        }

        // Handing a file back out, to somebody who already holds the channel key.
        "get" => {
            let Ok(Some(want)) = conn.recv::<Want>() else { return };
            match files::read_blob(&hello.channel, &want.hash) {
                None => {
                    let _ = conn.send(&Ack {
                        ok: false,
                        detail: "no such file on this channel, or it failed its own hash".into(),
                    });
                }
                Some(data) => {
                    if conn
                        .send(&Ack { ok: true, detail: data.len().to_string() })
                        .is_err()
                    {
                        return;
                    }
                    for chunk in data.chunks(files::CHUNK) {
                        if conn.send_raw(chunk).is_err() {
                            return;
                        }
                    }
                    let _ = conn.send_raw(&[]);
                }
            }
        }

        // Closing the room. The check is here rather than at the asking end:
        // holding the key proves you belong on the channel, not that you made
        // it, and only the machine that made it may close it.
        "delete" => {
            let ch = crate::channels::get(&hello.channel);
            let creator = ch.as_ref().map(|c| c.creator_name()).unwrap_or_default();
            let ack = if creator.is_empty() {
                Ack { ok: false, detail: format!("#{} records no creator", hello.channel) }
            } else if creator != hello.host {
                Ack {
                    ok: false,
                    detail: format!(
                        "#{} was made on {creator}; only {creator} can delete it",
                        hello.channel
                    ),
                }
            } else {
                let gone = history::purge(&hello.channel);
                files::forget_channel(&hello.channel); // its files go with it
                let _ = crate::channels::forget(&hello.channel);
                println!("deleted #{} ({gone} messages) at {}'s request", hello.channel, hello.host);
                Ack {
                    ok: true,
                    detail: format!("deleted #{}, {gone} message(s) removed", hello.channel),
                }
            };
            let _ = conn.send(&ack);
        }

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
