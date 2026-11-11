import WebSocket from 'ws';
import { randomUUID } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { Dispatcher } from './dispatcher.js';
import { ConfigurationRequired, type Paired } from './enrollment.js';
import { JournalCapacity, RecoveryRequired, type MappingCandidate, type MappingCursor } from './journal.js';
import { encodeFrames, FrameAssembler, LIMITS, PROTOCOL_VERSION, observationSequence, parseServer,
  type Delivery, type Identity, type Observation } from './protocol.js';

export type ConnectionStatus = 'offline' | 'connecting' | 'recovering' | 'connected' | 'authorization_required';
type Work = { delivery: Delivery; allowed: () => boolean; reply: (value: Observation) => void };

// The dispatcher outlives a socket. Network loss must not abandon native tracking
// or turn an ambiguous mutation into permission to submit it again.
export class Connector {
  private mappingCursor: MappingCursor | undefined;
  readonly dispatcher: Dispatcher;
  status: ConnectionStatus = 'offline';
  private readonly queue = new Map<string, Work>();
  constructor(dispatcher: Dispatcher) { this.dispatcher = dispatcher; }

  async run(paired: Paired, signal: AbortSignal): Promise<void> {
    const stop = new AbortController(); const lifetime = AbortSignal.any([signal, stop.signal]);
    const worker = this.work(lifetime).catch(error => { stop.abort(error); throw error; });
    // Attach immediately: cleanup always joins the worker, including a failure
    // while the network loop is inside a handshake or reconnect delay.
    void worker.catch(() => {});
    try {
      let attempt = 0;
      while (!lifetime.aborted) {
        try { await this.connect(paired, lifetime); attempt = 0; }
        catch (error) {
          if (lifetime.aborted) break;
          if (error instanceof ConfigurationRequired || error instanceof RecoveryRequired) throw error;
        }
        if (!lifetime.aborted) {
          this.status = 'offline';
          await delay(Math.min(30_000, 500 * 2 ** Math.min(attempt++, 6)) * (0.75 + Math.random() * 0.5), undefined, { signal: lifetime });
        }
      }
    } catch (error) { if (!lifetime.aborted) throw error; }
    finally { stop.abort(); await worker; this.queue.clear(); if (this.status !== 'authorization_required') this.status = 'offline'; }
  }

  private async work(signal: AbortSignal): Promise<void> {
    try {
      while (!signal.aborted) {
        const next = [...this.queue.entries()].find(([, work]) => this.dispatcher.journal.canAdmit(work.delivery.operation));
        if (next) {
          const [id, work] = next;
          try {
            const cached = await this.dispatcher.process(work.delivery, { signal, canDispatch: work.allowed });
            if (cached) work.reply(cached);
            this.queue.delete(id);
          } catch (error) {
            if (!(error instanceof JournalCapacity)) throw error;
            await this.observe(signal);
            await delay(250, undefined, { signal });
          }
        } else {
          await this.observe(signal);
          await delay(250, undefined, { signal });
        }
      }
    } catch (error) { if (!signal.aborted) throw error; }
  }

  private async observe(signal: AbortSignal): Promise<void> {
    try { await this.dispatcher.tick(signal); }
    catch (error) { if (!(error instanceof JournalCapacity)) throw error; }
  }

  private async connect(paired: Paired, signal: AbortSignal): Promise<void> {
    this.status = 'connecting';
    const journal = this.dispatcher.journal; const binding = paired.binding;
    const endpoint = new URL('/v1/local-machines/connect', binding.origin); endpoint.protocol = 'wss:';
    const socket = new WebSocket(endpoint, { headers: { authorization: `Bearer ${paired.credential}`,
      'x-openbot-installation-id': binding.installation_id, 'x-openbot-protocol-version': String(PROTOCOL_VERSION) },
      followRedirects: false, handshakeTimeout: LIMITS.handshakeMs, maxPayload: LIMITS.frame, perMessageDeflate: false });
    let identity: Identity | undefined; let allowed = false; let active = true;
    let lastReceived = performance.now(); let sent = journal.acknowledged();
    let frames = 0; let window = performance.now(); let failure: unknown;
    const assembler = new FrameAssembler(); const cached: Observation[] = [];
    let retirement: { nonce: string; candidates: MappingCandidate[]; expires: number } | undefined;
    const stop = new AbortController(); const connected = AbortSignal.any([signal, stop.signal]);
    const fail = (error: unknown) => { failure ??= error; allowed = false; stop.abort(); socket.terminate(); };
    const abort = () => fail(signal.reason ?? new Error('connector stopped'));
    signal.addEventListener('abort', abort, { once: true });
    socket.on('error', () => fail(new Error('Openbot connection failed')));
    socket.on('close', () => fail(new Error('Openbot connection closed')));
    socket.on('unexpected-response', (_request, response) => {
      response.destroy();
      if (response.statusCode === 401 || response.statusCode === 403) {
        this.status = 'authorization_required'; fail(new ConfigurationRequired('Machine authorization rejected; repair pairing'));
      } else fail(new Error('Openbot handshake rejected'));
    });
    socket.on('ping', () => { lastReceived = performance.now(); });
    socket.on('pong', () => { lastReceived = performance.now(); });
    socket.on('message', (data, binary) => {
      try {
        if (!active || binary) throw new Error('expected text frame');
        if (performance.now() - window >= 60_000) { frames = 0; window = performance.now(); }
        if (++frames > 1024) throw new Error('incoming frame budget exhausted');
        lastReceived = performance.now();
        const value = assembler.push(Buffer.isBuffer(data) ? data : Buffer.concat(Array.isArray(data) ? data : [Buffer.from(data)]));
        if (value === undefined) return;
        const message = parseServer(value);
        if (message.kind === 'welcome') {
          if (identity || message.welcome.identity.installation_id !== binding.installation_id
              || message.welcome.identity.credential_generation !== binding.credential_generation) throw new RecoveryRequired();
          if (message.welcome.recovery_required) { journal.requireRecovery(); throw new RecoveryRequired(); }
          journal.acknowledge(message.welcome.acknowledged_sequence);
          identity = message.welcome.identity; sent = journal.acknowledged(); this.status = 'recovering';
        } else {
          if (!identity || JSON.stringify(message.identity) !== JSON.stringify(identity)) throw new Error('stale connection identity');
          if (message.kind === 'acknowledged') journal.acknowledge(message.through);
          if (message.kind === 'ready') {
            allowed = message.ready && !journal.recoveryRequired(); this.status = allowed ? 'connected' : 'recovering';
          }
          if (message.kind === 'mappings_retired' && retirement?.nonce === message.nonce && performance.now() < retirement.expires) {
            journal.retireMappings(retirement.candidates, message.sessions);
            retirement = undefined;
          }
          if (message.kind === 'command') {
            const id = message.delivery.operation.id; const existing = this.queue.get(id);
            if (existing) {
              if (existing.delivery.operation.payload_digest !== message.delivery.operation.payload_digest) throw new Error('operation identity conflict');
              return;
            }
            const effect = ['create', 'send'].includes(message.delivery.operation.command.kind);
            if (this.queue.size >= (effect ? LIMITS.queued - LIMITS.controls : LIMITS.queued)) throw new Error('incoming command capacity');
            this.queue.set(id, { delivery: message.delivery, allowed: () => active && allowed,
              reply: value => { if (active && cached.length < LIMITS.queued) cached.push(value); } });
          }
        }
      } catch (error) { fail(error); }
    });
    const write = async (message: unknown) => {
      for (const frame of encodeFrames(message)) {
        connected.throwIfAborted();
        await new Promise<void>((resolve, reject) => {
          const cancel = () => { cleanup(); reject(new Error('socket write cancelled')); };
          const timer = setTimeout(() => { cleanup(); reject(new Error('socket write timed out')); socket.terminate(); }, LIMITS.ioMs);
          const cleanup = () => { clearTimeout(timer); connected.removeEventListener('abort', cancel); };
          connected.addEventListener('abort', cancel, { once: true });
          socket.send(frame, error => { cleanup(); if (error) reject(new Error('socket write failed')); else resolve(); });
        });
      }
    };
    try {
      const opened = performance.now();
      while (socket.readyState === WebSocket.CONNECTING) await delay(10, undefined, { signal: connected });
      connected.throwIfAborted();
      await write({ kind: 'hello', hello: { version: PROTOCOL_VERSION, installation_id: binding.installation_id,
        journal_id: binding.journal_id, journal_sequence: journal.sequence() } });
      while (!identity) {
        if (performance.now() - opened >= LIMITS.handshakeMs) throw new Error('welcome timed out');
        await delay(10, undefined, { signal: connected });
      }
      let heartbeat = 0; let ready = 0; let compaction = 0; let retention = 0;
      while (!connected.aborted) {
        assembler.expire();
        const now = performance.now();
        if (retirement && now >= retirement.expires) retirement = undefined;
        if (now - lastReceived >= 45_000) throw new Error('Openbot heartbeat expired');
        if (now - heartbeat >= LIMITS.heartbeatMs) { await write({ kind: 'heartbeat', identity }); heartbeat = now; }
        const observations = journal.outbox(sent);
        for (const observation of observations) {
          // Keep at most one bounded page unacknowledged on this transport.
          if (sent - journal.acknowledged() >= LIMITS.queued) break;
          await write({ kind: 'observation', identity, observation }); sent = observationSequence(observation);
        }
        for (const observation of cached.splice(0)) await write({ kind: 'observation', identity, observation });
        if (now >= compaction) { journal.compactAcknowledged(); compaction = now + 60_000; }
        if (allowed && now >= retention && !retirement) {
          let candidates = journal.mappingCandidates(this.mappingCursor);
          if (!candidates.length && this.mappingCursor) candidates = journal.mappingCandidates();
          const last = candidates.at(-1);
          // Advance even on rejection/lost replies, and across socket reconnects.
          // Only a matching live response can delete a mapping; wrap revisits loss.
          this.mappingCursor = last && { last_used_at: last.last_used_at, id: last.id };
          if (candidates.length) {
            retirement = { nonce: randomUUID(), candidates, expires: now + LIMITS.ioMs };
            await write({ kind: 'retire_mappings', identity, request: {
              nonce: retirement.nonce, sessions: candidates.map(candidate => candidate.id),
            } });
          }
          retention = now + 60_000;
        }
        if (sent === journal.sequence() && journal.acknowledged() === sent && now - ready >= 500) {
          await write({ kind: 'ready', identity }); ready = now;
        }
        await delay(25, undefined, { signal: connected });
      }
    } catch (error) { throw failure ?? error; }
    finally {
      active = false; allowed = false; stop.abort(); signal.removeEventListener('abort', abort);
      socket.terminate();
    }
  }
}
