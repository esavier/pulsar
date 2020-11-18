// #![allow(unused_code)]
// todo yeah i know
#![allow(dead_code)]
#![allow(clippy::while_let_loop)]
// #![allow(unused_imports)]
// #![allow(non_snake_case)]
// #![allow(unused_must_use)]
// #![allow(unused_variables)]

use libp2p::{
  identity,
  // identify, [todo]
  PeerId,
  gossipsub::{
    self,
    protocol::MessageId,
    GossipsubEvent,
    GossipsubMessage,
    MessageAuthenticity,
    Topic,
  }
};
use std::{
  error::Error,
  task::{Context, Poll},
};

use std::time::Instant;
use async_std::task;
use futures::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use serde::{Serialize,Deserialize};
use std::collections::VecDeque;
mod logger;

// -------------------------------------------------------
//        structs & impls
// -------------------------------------------------------
// [todo]: move this stuff to another module

#[derive(Serialize, Deserialize)]
struct PeerEntry {
  isreq: bool,
  id: uuid::Uuid,
  tags: String,
}

impl PeerEntry {
  fn make_request(v: String) -> PeerEntry {
    PeerEntry {
      isreq: true,
      id: uuid::Uuid::new_v4(),
      tags: v,
    }
  }

  fn make_response(&self, v: String) -> PeerEntry {
    PeerEntry {
      isreq: false,
      id: self.id,
      tags: v,
    }
  }
}

// [todo]: impl me default
struct ResponseTimer {
  id: uuid::Uuid,
  date: Instant,
}

struct ReseponeHandler {
  responses: VecDeque<ResponseTimer>,
}

impl ReseponeHandler {
  fn prune(&mut self) {
    loop {
      if self.responses.is_empty() { return }
      if self.responses.front().unwrap().date.elapsed().as_secs() > 5 {
        self.responses.pop_front();
      } else {
        return
      }
    }
  }

  pub fn check(self, v: &PeerEntry) -> bool {
    for each in &self.responses {
      if each.id == v.id {
        return true;
      }
    }
    false
  }

  pub fn wait_for(&mut self, v: &PeerEntry) {
    let r =  ResponseTimer {
      id: v.id,
      date: Instant::now(),
    };
    self.responses.push_back(r)
  }

  pub fn new() -> ReseponeHandler {
    ReseponeHandler{
      responses: VecDeque::new(),
    }
  }
}


// todo: hook up identity/identify
// todo: hook up mdns
// todo: unix socket to interface
// todo: dadmonize
// todo: configuration and switches

// right now its is proof of concept hacked together and
// moved to work from mio to libp2p
// much testing and further development is needed

// -------------------------------------------------------
//        main
// -------------------------------------------------------

fn main() -> Result<(), Box<dyn Error>> {
  logger::setup_logging().unwrap();

  let peer_privk = identity::Keypair::generate_ed25519();
  // let peer_pubk  = peer_privk.public();
  let peer_id    = PeerId::from(peer_privk.public());
  let transport  = libp2p::build_development_transport(peer_privk.clone())?;

  // [todo]: add configuration for domain topic
  // [todo]: add credentials?
  let topic = Topic::new("erglabs.org".into());

  logger::info!("Local peer id: {:?}", peer_id);
  let mut swarm = {
    let message_id_fn = |message: &GossipsubMessage| {
      let mut s = DefaultHasher::new();
      message.data.hash(&mut s);
      MessageId::from(s.finish().to_string())
    };

    let gossipsub_config = gossipsub::GossipsubConfigBuilder::new()
      .heartbeat_interval(Duration::from_secs(10))
      .message_id_fn(message_id_fn)
      .build();

    // [todo] sign with my privkey, consider HMAC ?
    let mut gossipsub =
      gossipsub::Gossipsub::new(MessageAuthenticity::Signed(peer_privk), gossipsub_config);
    gossipsub.subscribe(topic.clone());
    libp2p::Swarm::new(transport, gossipsub, peer_id)
  };

  // libp2p::Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/0".parse().unwrap()).unwrap();
  match libp2p::Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/36000".parse().unwrap()) {
    Ok(v) => v,
    // yeah i know, this handler is required for testing purposes, fuck off
    _ => libp2p::Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/36001".parse().unwrap()).unwrap(),
  };


  // [todo][✓]: add configurable dialing
  // [todo]: add mdns search
  if let Some(to_dial) = std::env::args().nth(1) {
    let dialing = to_dial.clone();
    match to_dial.parse() {
      Ok(to_dial) => match libp2p::Swarm::dial_addr(&mut swarm, to_dial) {
        Ok(_) => logger::info!("Dialed {:?}", dialing),
        // thats ok if we fail
        Err(e) => logger::info!("Dial {:?} failed: {:?}", dialing, e),
      },
      // but this is serious
      Err(err) => logger::error!("Failed to parse address to dial: {:?}", err),
    }
  }

  // console handler for now, to remove later with demonization
  // let mut stdin = io::BufReader::new(io::stdin()).lines();

  // [todo]: this will be in config
  let mut taglist: Vec<String> = Vec::new();
  taglist.push("erglabs.org".to_owned());

  let mut listening = false;
  let msg = PeerEntry::make_request("erglabs.org".to_owned());
  swarm.publish(&topic, bincode::serialize(&msg).unwrap()).unwrap();

  task::block_on(future::poll_fn(move |cx: &mut Context<'_>| {
    loop {
      match swarm.poll_next_unpin(cx) {
        Poll::Ready(Some(gossip_event)) => match gossip_event {
          GossipsubEvent::Message(peer_id, id, message) => {
            let msg: PeerEntry = bincode::deserialize(&message.data[..]).unwrap();
            logger::info!("=================\n[id]{:?}[peer]{:?}\n[id]: {}\n[msg]: {}\n=================",
              id,
              peer_id,
              msg.id.to_hyphenated(),
              msg.tags,
            );
            match msg.isreq {
              true => {
                if taglist.contains(&msg.tags) {
                  swarm.publish(&topic, bincode::serialize(&msg.make_response("responding!".to_owned())).unwrap()).unwrap();
                };
              },
              false => {},
            }
          },
          _ => { println!("weird message incoming, ignoring" );}
        },
        Poll::Ready(None) | Poll::Pending => break,
      }
    }

    if !listening {
      for addr in libp2p::Swarm::listeners(&swarm) {
        println!("Listening on {:?}", addr);
        listening = true;
      }
    }

    Poll::Pending
  }))
}
