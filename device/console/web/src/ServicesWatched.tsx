import { useCallback, useEffect, useRef, useState } from "react";
import { addWatch, monitorFailure, probeNow, readMonitors, removeWatch, type MonitorTarget } from "./monitorApi";
import type { Grant } from "./terminal";

/**
 * **THE SERVICES THIS MACHINE WATCHES (the hundred-and-seventy-second list).**
 *
 * The device has probed `host:port` targets on its own timer since the monitor plugin landed, and the console — which is where
 * an operator looks at a machine — never showed them. Measured 2026-11-12: of the device's 46 tools the console wired 32, and
 * the monitor family was five of the missing fifteen. A service that went down ten minutes ago is exactly the fact this page
 * exists to surface, and it was only reachable by opening a terminal on the machine.
 *
 * Adding persists a watch on the machine. Probing records a new sample; stopping discards its series.
 * The page names those consequences before the operator acts.
 */
export function ServicesWatched(props: { grant: Grant; online?: boolean }) {
  const [targets, setTargets] = useState<MonitorTarget[] | null>(null);
  const [interval, setInterval] = useState<number | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [reading, setReading] = useState(false);
  const generation = useRef(0);
  const epoch = useRef(0);
  const previousGrant = useRef(props.grant);
  // **A WATCH IS A WRITE THAT OUTLIVES THE CALLER (the hundred-and-seventy-fourth list).** `monitor.add` persists on the machine
  // and `monitor.remove` DISCARDS the series, so the page names both facts where the operator can read them, and removing asks
  // first. The form's state is dropped once the add lands.
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [path, setPath] = useState("");
  const [writing, setWriting] = useState(false);
  const [writeError, setWriteError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [probing, setProbing] = useState<string | null>(null);

  const probe = async (id: string) => {
    const mine = epoch.current;
    setProbing(id);
    setWriteError(null);
    try {
      await probeNow(props.grant, id);
      if (epoch.current !== mine) return;
      await read();
    } catch (error) {
      if (epoch.current !== mine) return;
      setWriteError(monitorFailure(error).replace(/^The services could not be read — /, ""));
    } finally {
      if (epoch.current === mine) setProbing(null);
    }
  };

  const add = async (event: React.FormEvent) => {
    event.preventDefault();
    const wantedPort = Number(port);
    if (host.trim() === "" || !Number.isInteger(wantedPort) || wantedPort < 1 || wantedPort > 65535) {
      setWriteError("Name the host and a port between 1 and 65535.");
      return;
    }
    const mine = epoch.current;
    setWriting(true);
    setWriteError(null);
    setNotice(null);
    try {
      await addWatch(props.grant, { host: host.trim(), port: wantedPort, path: path.trim() });
      if (epoch.current !== mine) return;
      setNotice(`Watching ${host.trim()}:${wantedPort}${path.trim() === "" ? "" : path.trim()} — the machine probes it every 15 s.`);
      setHost("");
      setPort("");
      setPath("");
      await read();
    } catch (error) {
      if (epoch.current !== mine) return;
      setWriteError(monitorFailure(error).replace(/^The services could not be read — /, ""));
    } finally {
      if (epoch.current === mine) setWriting(false);
    }
  };

  const stop = async (id: string) => {
    const mine = epoch.current;
    setWriting(true);
    setWriteError(null);
    setNotice(null);
    try {
      await removeWatch(props.grant, id);
      if (epoch.current !== mine) return;
      setNotice(`Stopped watching ${id}. Its series is gone.`);
      setConfirming(null);
      await read();
    } catch (error) {
      if (epoch.current !== mine) return;
      setWriteError(monitorFailure(error).replace(/^The services could not be read — /, ""));
    } finally {
      if (epoch.current === mine) setWriting(false);
    }
  };

  const read = useCallback(async () => {
    const mine = ++generation.current;
    setReading(true);
    try {
      const answer = await readMonitors(props.grant);
      if (generation.current !== mine) return;
      setTargets(answer.targets);
      setInterval(answer.intervalSecs);
      setFailed(null);
    } catch (error) {
      if (generation.current !== mine) return;
      setFailed(monitorFailure(error));
      setTargets(null);
    } finally {
      if (generation.current === mine) setReading(false);
    }
  }, [props.grant]);

  // **A MACHINE THE FLEET SAYS IS AWAY IS NOT ASKED (the forty-third list's rule, applied here).** A read against an offline
  // machine is a request that can only time out, and the notice it produces would say the same thing the fleet already knows.
  useEffect(() => {
    epoch.current += 1;
    generation.current += 1;
    setReading(false);
    setWriting(false);
    setProbing(null);
    setConfirming(null);
    if (previousGrant.current !== props.grant) {
      previousGrant.current = props.grant;
      setTargets(null);
      setInterval(null);
      setFailed(null);
      setHost("");
      setPort("");
      setPath("");
      setWriteError(null);
      setNotice(null);
    }
    if (props.online !== false) void read();
    return () => {
      epoch.current += 1;
      generation.current += 1;
    };
  }, [read, props.grant, props.online]);

  return (
    <section className="system-section machine-watches" aria-labelledby="system-services">
      <header className="panel-head">
        <h3 id="system-services">Services this machine watches</h3>
      </header>
      <p className="hint">The machine keeps these watches running. Probe now records a new sample; stopping a watch discards its series.</p>
      {/* **THE TWO FACTS A WRITE OWES THE READER.** A watch is PERSISTED on the machine (it outlives this page), and stopping one
          DISCARDS its series — both said here, in the form and in the confirmation, not in a tool description nobody reads. */}
      <div className="service-workspace">
        <aside className="service-compose" aria-label="Add a service watch">
      {props.online !== false && (
        <details className="card-details service-add">
          <summary>Watch a host and port</summary>
          <form className="service-form" onSubmit={add}>
            <label>
              Host to watch
              <input value={host} onChange={(event) => setHost(event.target.value)} placeholder="127.0.0.1" />
            </label>
            <label>
              Port to watch
              <input
                value={port}
                onChange={(event) => setPort(event.target.value)}
                inputMode="numeric"
                placeholder="19000"
              />
            </label>
            <label>
              Path to watch <span className="muted">(optional — makes it an HTTP GET)</span>
              <input value={path} onChange={(event) => setPath(event.target.value)} placeholder="/health" />
            </label>
            <button type="submit" className="btn" disabled={writing}>
              {writing ? "Saving…" : "Start watching"}
            </button>
            <p className="hint">
              The watch lives on the machine and survives a restart. The console only asks it to be made.
            </p>
          </form>
        </details>
      )}
        </aside>
        <div className="service-observations">
      {notice !== null && (
        <p className="hint service-notice" role="status">
          {notice}
        </p>
      )}
      {writeError !== null && (
        <p className="error" role="alert">
          {writeError}
        </p>
      )}
      {failed !== null && (
        <div className="notice" role="status">
          <span>{failed}</span>
          <button type="button" disabled={reading || props.online === false} onClick={() => void read()}>
            {reading ? "Retrying…" : "Retry"}
          </button>
        </div>
      )}
      {reading && targets !== null && props.online !== false && (
        <p className="hint" role="status">Refreshing the watch list. The previous read stays below.</p>
      )}
      {targets === null && failed === null && props.online !== false && (
        <p className="hint" role="status">
          Reading the watch list…
        </p>
      )}
      {targets === null && failed === null && props.online === false && (
        <p className="hint">The machine is offline, so its watch list cannot be read.</p>
      )}
      {targets !== null && targets.length === 0 && (
        <p className="hint">This machine watches no services.</p>
      )}
      {targets !== null && targets.length > 0 && (
        <>
          <p className="hint">
            {targets.length === 1 ? "One target" : `${targets.length} targets`}
            {interval !== null ? ` · probed every ${interval} s on the machine` : ""}
          </p>
          <ul className="service-list">
            {targets.map((target) => (
              <li key={target.id} className="service-row">
                <span className="service-address mono">
                  {target.host}:{target.port}
                  {target.path !== null ? target.path : ""}
                </span>
                {stateWord(target)}
                {target.summary.sinceMs !== null && target.summary.upNow !== null && (
                  <span className="service-meta">
                    {target.summary.upNow ? "up" : "down"} since {stamp(target.summary.sinceMs)}
                  </span>
                )}
                {/* **A PERCENTAGE WITHOUT A COUNT IS NOT A CLAIM THIS PAGE CAN MAKE.** The shipped device's summary carries
                    `up_pct` but no `probes`; drawing `0% of 0 probes` invented the zero. The count is named only when the build
                    reports one. */}
                {target.summary.upPct !== null && target.summary.probes !== null && target.summary.probes > 0 && (
                  <span className="service-meta">
                    {Math.round(target.summary.upPct)}% of {target.summary.probes} probes
                  </span>
                )}
                {target.summary.upPct !== null && target.summary.probes === null && (
                  <span className="service-meta">{Math.round(target.summary.upPct)}% up in the window</span>
                )}
                {target.summary.drops !== null && target.summary.drops > 0 && (
                  <span className="service-meta">
                    {target.summary.drops} {target.summary.drops === 1 ? "drop" : "drops"}
                  </span>
                )}
                {target.summary.lastStatus !== null && (
                  <span className="service-meta">HTTP {target.summary.lastStatus}</span>
                )}
                <span className="service-actions">
                <button
                  type="button"
                  className="link service-probe"
                  title="Take a sample now and record it in the target's series"
                  disabled={probing === target.id || props.online === false}
                  onClick={() => void probe(target.id)}
                >
                  {probing === target.id ? "Probing…" : "Probe now"}
                </button>
                {confirming === target.id ? (
                  <span className="service-confirm">
                    Stop watching {target.id}? Its series is discarded.{" "}
                    <button type="button" className="link" onClick={() => setConfirming(null)}>
                      Keep
                    </button>{" "}
                    <button type="button" className="link danger" disabled={writing || props.online === false} onClick={() => void stop(target.id)}>
                      Stop
                    </button>
                  </span>
                ) : (
                  <button type="button" className="link service-stop" disabled={writing || props.online === false} onClick={() => setConfirming(target.id)}>
                    Stop watching
                  </button>
                )}
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
      {props.online === false && targets !== null && (
        <p className="hint">
          The machine is offline — this is the last watch list the console could read.
        </p>
      )}
        </div>
      </div>
    </section>
  );
}

/**
 * The state as a WORD, and a null as a different word. `up_now` is null until the first probe has happened, and drawing that
 * as "down" would be the one thing this page must not do: call a service broken that nobody has measured.
 */
function stateWord(target: MonitorTarget): React.ReactNode {
  const s = target.summary;
  if (s.upNow === null) return <span className="chip">Not probed</span>;
  return s.upNow ? (
    <span className="chip chip-ok">Up</span>
  ) : (
    <span className="chip chip-bad">Down</span>
  );
}

/** The device's stamps are Unix MILLISECONDS here (`since_ms`); the page prints the local time. */
function stamp(ms: number): string {
  const at = new Date(ms);
  if (Number.isNaN(at.getTime())) return "an unknown moment";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(at.getHours())}:${pad(at.getMinutes())}`;
}
