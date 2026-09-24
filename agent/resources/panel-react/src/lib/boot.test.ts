// Coverage audit row 17: computeBoot is the ONE place the SPA decides
// host/token/connected — its precedence chain (injected > ?token= > stored)
// and proxy-mode non-persistence carry two past regressions (round-139
// blank-viewport 401 loop; round-122/124 plaintext token on the console
// origin). None of it had a test.
import { describe, it, expect, vi, beforeEach } from "vitest";

const initTransport = vi.fn();
vi.mock("./api", () => ({
  initTransport: (...a: unknown[]) => initTransport(...a),
}));

const setLocation = (pathname: string, search = "", host = "d1.test") => {
  vi.stubGlobal("location", {
    pathname,
    search,
    host,
    origin: `https://${host}`,
  });
};

describe("computeBoot", () => {
  let computeBoot: (f: () => void) => {
    host: string;
    tok: string;
    connected: boolean;
  };
  beforeEach(async () => {
    initTransport.mockClear();
    localStorage.clear();
    delete (window as any).__PANEL_TOKEN__;
    vi.resetModules();
    vi.stubGlobal("location", undefined as any); // re-stubbed per test
    ({ computeBoot } = await import("./boot"));
  });

  it("desktop injected token wins and seeds BOTH transport and state", () => {
    setLocation("/desktop/");
    (window as any).__PANEL_TOKEN__ = "inj";
    localStorage.setItem("summriseToken", "stale");
    const boot = computeBoot(() => {});
    expect(boot.tok).toBe("inj");
    expect(boot.connected).toBe(true);
    expect(initTransport).toHaveBeenCalledWith(
      "d1.test",
      "inj",
      expect.anything(),
    );
  });

  it("?token= beats stored when nothing is injected", () => {
    setLocation("/panel/", "?token=urltok");
    localStorage.setItem("summriseToken", "stale");
    const boot = computeBoot(() => {});
    expect(boot.tok).toBe("urltok");
    expect(boot.connected).toBe(true);
  });

  it("stored token is the fallback", () => {
    setLocation("/panel/");
    localStorage.setItem("summriseToken", "stale");
    expect(computeBoot(() => {}).tok).toBe("stale");
  });

  it("proxy mode: cookie credential — never persists the token, deletes stale", () => {
    setLocation("/proxy/panel", "?token=ptok", "console.test");
    localStorage.setItem("summriseToken", "stale-leftover");
    const boot = computeBoot(() => {});
    expect(boot.tok).toBe("ptok");
    expect(localStorage.getItem("summriseToken")).toBeNull(); // R122/124 contract
  });

  it("same-origin with NO token shows the conn form (not a dead 401 loop)", () => {
    setLocation("/panel/");
    const boot = computeBoot(() => {});
    expect(boot.connected).toBe(false);
  });

  it("private-mode localStorage (setItem throws) must not crash boot", () => {
    setLocation("/desktop/");
    (window as any).__PANEL_TOKEN__ = "inj";
    const real = Storage.prototype.setItem;
    Storage.prototype.setItem = () => {
      throw new Error("quota");
    };
    try {
      const boot = computeBoot(() => {});
      expect(boot.tok).toBe("inj"); // session-only, still boots
    } finally {
      Storage.prototype.setItem = real;
    }
  });

  // THE LINK ROUND-88'S RECOVERY DEPENDS ON, and the one thing every test above left as `() => {}`.
  // `computeBoot` does not SEE a 401 — the transport does — so what it must do is hand the callback
  // along, at BOTH call sites. Drop this and a rotated token 401s into a noop: the panel stays
  // "connected" with everything dead and the conn form unreachable, which is the bug round-88 fixed.
  // (The recovery itself — the flag flipping `connected` to false — lives in App.tsx as a render block
  // and is still not covered by a test; this pins the half that can be.)
  it("hands the 401 callback to the transport on the same-origin path", () => {
    setLocation("/desktop/");
    (window as any).__PANEL_TOKEN__ = "inj";
    const onAuthFail = vi.fn();
    computeBoot(onAuthFail);
    expect(initTransport).toHaveBeenCalledWith("d1.test", "inj", onAuthFail);
  });

  it("hands it over on the stored-token path too", () => {
    setLocation("/somewhere/else/");
    localStorage.setItem("summriseHost", "h.test");
    localStorage.setItem("summriseToken", "stored");
    const onAuthFail = vi.fn();
    computeBoot(onAuthFail);
    expect(initTransport).toHaveBeenCalledWith("h.test", "stored", onAuthFail);
  });
});
