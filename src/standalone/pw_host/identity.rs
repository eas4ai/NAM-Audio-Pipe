// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 Fábio Henrique de Lima Silva (fhl.bsb@gmail.com) All rights reserved.

//! Constantes centralizadas de identidade do produto para o grafo PipeWire.
//!
//! Todos os nomes de nós, streams, grupos e thread loop devem ser referenciados
//! exclusivamente a partir deste módulo — nunca como string literals inline.

/// Nome do nó de captura (Virtual Sink). Visível em patchbays (qpwgraph, Helvum).
pub const PW_CAPTURE_NODE_NAME: &str = "NAM-Audio-Pipe-input";
/// Descrição do nó de captura. Visível em patchbays.
pub const PW_CAPTURE_NODE_DESC: &str = "NAM-Audio-Pipe Input";
/// Nome do stream de captura passado ao construtor `StreamBox::new`.
pub const PW_CAPTURE_STREAM_NAME: &str = "NAM-Audio-Pipe";

/// Nome do nó de playback. Visível em patchbays.
pub const PW_PLAYBACK_NODE_NAME: &str = "NAM-Audio-Pipe-playback";
/// Descrição do nó de playback. Visível em patchbays.
pub const PW_PLAYBACK_NODE_DESC: &str = "NAM-Audio-Pipe Processed Output";
/// Nome do stream de playback passado ao construtor `StreamBox::new`.
pub const PW_PLAYBACK_STREAM_NAME: &str = "NAM-Audio-Pipe-Output";

/// `node.group` — garante que os dois streams sejam agendados pelo mesmo driver.
pub const PW_NODE_GROUP: &str = "nam-audio-pipe-dsp";
/// `node.link-group` — mantém os dois streams no mesmo grupo de links.
pub const PW_LINK_GROUP: &str = "nam-audio-pipe-link-group";
/// Nome do PipeWire thread loop (interno, não visível ao usuário).
pub const PW_THREAD_LOOP_NAME: &str = "nam-audio-pipe-loop";
