// The console's HTTP surface is the one place every page talks to the device through, so these
// tests pin the contract the pages rely on: the token header, the JSON and content-type rules, the
// path and method each endpoint reaches, and the two error classes.
//
// `fetch` is the only global replaced; it is restored in `afterEach` so no later file inherits the
// stub, and the module's token is cleared so header assertions never leak between tests.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  api,
  getToken,
  HttpError,
  meshReadiness,
  meshStatus,
  NoAnswer,
  setToken
} from "./api";

type Reply = { status?: number; statusText?: string; text?: string; json?: unknown };

/** The part of a Response `call` reads: enough to be an answer, and nothing more. */
function answer(reply: Reply = {}): Response {
  const status = reply.status ?? 200;
  const body = reply.text ?? (reply.json === undefined ? "" : JSON.stringify(reply.json));
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText: reply.statusText ?? "OK",
    text: async () => body
  } as Response;
}

let fetchMock: ReturnType<typeof vi.fn>;

function lastRequest(): { path: string; init: RequestInit } {
  expect(fetchMock).toHaveBeenCalledTimes(1);
  const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit];
  return { path, init };
}

/** Run one successful call and return the `init` the client handed to fetch. */
async function request(
  call: () => Promise<unknown>,
  expectedPath: string,
  method?: string
): Promise<RequestInit> {
  fetchMock.mockResolvedValue(answer({ text: "null" }));
  await call();
  const { path, init } = lastRequest();
  expect(path).toBe(expectedPath);
  expect(init.method).toBe(method);
  return init;
}

function headers(init: RequestInit): Record<string, string> {
  return init.headers as Record<string, string>;
}

/** The rejection value, so the test can assert on the error without calling twice. */
async function failureOf(promise: Promise<unknown>): Promise<unknown> {
  try {
    await promise;
  } catch (error) {
    return error;
  }
  throw new Error("expected the call to reject");
}

beforeEach(() => {
  setToken(null);
  fetchMock = vi.fn();
  vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
  setToken(null);
});

describe("the token", () => {
  it("is empty before one is set, the value after, and empty again after null", () => {
    expect(getToken()).toBe("");
    setToken("session-token");
    expect(getToken()).toBe("session-token");
    setToken(null);
    expect(getToken()).toBe("");
  });
});

describe("the request the client builds", () => {
  it("sends no authorization header before a token, and no content-type on a GET", async () => {
    const init = await request(() => api.session(), "/session");
    expect(headers(init)).toEqual({});
  });

  it("sends a token as a Bearer header, still without a GET content-type", async () => {
    setToken("session-token");
    const init = await request(() => api.session(), "/session");
    expect(headers(init)).toEqual({ authorization: "Bearer session-token" });
  });

  it("declares JSON and sends exactly the body a POST passes", async () => {
    setToken("session-token");
    const init = await request(() => api.signIn({ code: "ABC123" }), "/auth", "POST");
    expect(headers(init)).toEqual({
      authorization: "Bearer session-token",
      "content-type": "application/json"
    });
    expect(init.body).toBe(JSON.stringify({ code: "ABC123" }));
  });

  it("has no content-type when a POST carries no body", async () => {
    const init = await request(() => api.enroll(), "/enroll", "POST");
    expect(headers(init)).toEqual({});
    expect(init.body).toBeUndefined();
  });
});

describe("the answers", () => {
  it("parses a JSON 200", async () => {
    fetchMock.mockResolvedValue(answer({ json: { authenticated: true, email: "ops@example" } }));
    await expect(api.session()).resolves.toEqual({
      authenticated: true,
      email: "ops@example"
    });
  });

  it("calls a non-JSON 200 the edge's answer, not a parse failure", async () => {
    fetchMock.mockResolvedValue(answer({ text: "<html>Access login</html>" }));
    await expect(api.session()).rejects.toBeInstanceOf(NoAnswer);
  });

  it("throws HttpError carrying the body's error code on a JSON failure", async () => {
    fetchMock.mockResolvedValue(
      answer({ status: 403, statusText: "Forbidden", json: { error: "grant_denied" } })
    );
    const failure = await failureOf(api.session());
    expect(failure).toBeInstanceOf(HttpError);
    expect(failure).toMatchObject({ status: 403, code: "grant_denied" });
  });

  it("falls back to statusText when a failing answer is empty", async () => {
    fetchMock.mockResolvedValue(answer({ status: 503, statusText: "Service Unavailable" }));
    const failure = await failureOf(api.session());
    expect(failure).toBeInstanceOf(HttpError);
    expect(failure).toMatchObject({ status: 503, code: "Service Unavailable" });
  });

  it("also falls back to statusText when a JSON failure names no error", async () => {
    fetchMock.mockResolvedValue(
      answer({ status: 404, statusText: "Not Found", json: { detail: "no such route" } })
    );
    const failure = await failureOf(api.session());
    expect(failure).toBeInstanceOf(HttpError);
    expect(failure).toMatchObject({ status: 404, code: "Not Found" });
  });

  it("calls a non-JSON error page the edge's answer, not HttpError", async () => {
    // A proxy or an Access login page can carry any status; no JSON still means the edge, not this
    // client, answered.
    fetchMock.mockResolvedValue(
      answer({ status: 502, statusText: "Bad Gateway", text: "<html>proxy error</html>" })
    );
    await expect(api.session()).rejects.toBeInstanceOf(NoAnswer);
  });
});

describe("the endpoints", () => {
  it("session GETs /session", async () => {
    await request(() => api.session(), "/session");
  });

  it("signIn POSTs the credential to /auth", async () => {
    const init = await request(() => api.signIn({ token: "credential" }), "/auth", "POST");
    expect(init.body).toBe(JSON.stringify({ token: "credential" }));
  });

  it("devices reads /fleet, not /devices", async () => {
    await request(() => api.devices(), "/fleet");
  });

  it("health GETs /health", async () => {
    await request(() => api.health(), "/health");
  });

  it("audit asks for a limit of 20 by default", async () => {
    await request(() => api.audit(), "/audit?limit=20");
  });

  it("audit passes the caller's limit", async () => {
    await request(() => api.audit(5), "/audit?limit=5");
  });

  it("rename POSTs the name to /fleet/<id>/name", async () => {
    const init = await request(
      () => api.rename("dev-1", "workbench"),
      "/fleet/dev-1/name",
      "POST"
    );
    expect(init.body).toBe(JSON.stringify({ name: "workbench" }));
  });

  it("remove DELETEs /fleet/<id>", async () => {
    const init = await request(() => api.remove("dev-1"), "/fleet/dev-1", "DELETE");
    expect(init.body).toBeUndefined();
  });

  it("connections GETs the device's saved connections", async () => {
    await request(() => api.connections("dev-1"), "/fleet/dev-1/connections");
  });

  it("saveConnection POSTs the draft to the device's connections", async () => {
    const draft = { name: "shop shell", kind: "ssh", host: "10.0.0.5", port: 22, user: "ops" };
    const init = await request(
      () => api.saveConnection("dev-1", draft),
      "/fleet/dev-1/connections",
      "POST"
    );
    expect(init.body).toBe(JSON.stringify(draft));
  });

  it("removeConnection DELETEs one saved connection", async () => {
    await request(
      () => api.removeConnection("dev-1", "conn-1"),
      "/fleet/dev-1/connections/conn-1",
      "DELETE"
    );
  });

  it("enroll POSTs to /enroll", async () => {
    await request(() => api.enroll(), "/enroll", "POST");
  });

  it("grant POSTs exactly the device, tools, lifetime and destructive flag", async () => {
    const init = await request(
      () => api.grant("dev-1", ["read_file", "run_command"], 300, true),
      "/grants",
      "POST"
    );
    expect(init.body).toBe(
      JSON.stringify({
        device_id: "dev-1",
        tools: ["read_file", "run_command"],
        expires_in_s: 300,
        allow_destructive: true
      })
    );
  });

  it("tokens GETs /tokens and parses the metadata list", async () => {
    const meta = {
      id: "tok-1",
      name: "laptop-dsh",
      device_id: "dev-1",
      tools: ["files.read", "host.facts"],
      allow_destructive: false,
      created_at_ms: 1
    };
    fetchMock.mockResolvedValue(answer({ json: [meta] }));
    await expect(api.tokens()).resolves.toEqual([meta]);
    const { path, init } = lastRequest();
    expect(path).toBe("/tokens");
    expect(init.method).toBeUndefined();
  });

  it("createToken POSTs exactly the name, device, tools and destructive flag", async () => {
    const init = await request(
      () =>
        api.createToken({
          name: "laptop-dsh",
          device_id: "dev-1",
          tools: ["files.read", "host.facts"],
          allow_destructive: true
        }),
      "/tokens",
      "POST"
    );
    expect(init.body).toBe(
      JSON.stringify({
        name: "laptop-dsh",
        device_id: "dev-1",
        tools: ["files.read", "host.facts"],
        allow_destructive: true
      })
    );
  });

  it("revokeToken DELETEs /tokens/<id> with no body", async () => {
    const init = await request(() => api.revokeToken("tok-1"), "/tokens/tok-1", "DELETE");
    expect(init.body).toBeUndefined();
  });

  it("rotateToken POSTs /tokens/<id>/rotate and parses the created shape with the value", async () => {
    const created = {
      id: "tok-2",
      name: "laptop-dsh",
      device_id: "dev-1",
      tools: ["files.read", "host.facts"],
      allow_destructive: false,
      created_at_ms: 2,
      token: "srm_" + "b".repeat(64)
    };
    fetchMock.mockResolvedValue(answer({ json: created }));
    await expect(api.rotateToken("tok-1")).resolves.toEqual(created);
    const { path, init } = lastRequest();
    expect(path).toBe("/tokens/tok-1/rotate");
    expect(init.method).toBe("POST");
    // The rotate request names the token in the path and nothing else: the new scope is the old one.
    expect(init.body).toBeUndefined();
  });

  it("tools GETs the registry with the token and parses the names", async () => {
    setToken("session-token");
    fetchMock.mockResolvedValue(
      answer({ json: [{ name: "files.read" }, { name: "host.facts" }] })
    );
    await expect(api.tools()).resolves.toEqual([{ name: "files.read" }, { name: "host.facts" }]);
    const { path, init } = lastRequest();
    expect(path).toBe("/tools");
    expect(init.method).toBeUndefined();
    expect(headers(init)).toEqual({ authorization: "Bearer session-token" });
  });

  it("meshStatus GETs /mesh", async () => {
    await request(() => meshStatus(), "/mesh");
  });
});

describe("meshReadiness", () => {
  it("reads /fleet/<id>/mesh and returns the broker's answer", async () => {
    fetchMock.mockResolvedValue(
      answer({ json: { configured: true, reachable: true, candidates: 3 } })
    );
    await expect(meshReadiness("dev-1")).resolves.toEqual({
      configured: true,
      reachable: true,
      candidates: 3
    });
    expect(lastRequest().path).toBe("/fleet/dev-1/mesh");
  });

  it("encodes the device id in the path", async () => {
    fetchMock.mockResolvedValue(answer({ text: "null" }));
    await meshReadiness("dev one/two");
    expect(lastRequest().path).toBe("/fleet/dev%20one%2Ftwo/mesh");
  });

  it("maps no_mesh_broker to configured: false", async () => {
    fetchMock.mockResolvedValue(
      answer({ status: 409, statusText: "Conflict", json: { error: "no_mesh_broker" } })
    );
    await expect(meshReadiness("dev-1")).resolves.toEqual({ configured: false });
  });

  it("maps any other HttpError to its code", async () => {
    fetchMock.mockResolvedValue(
      answer({ status: 502, statusText: "Bad Gateway", json: { error: "broker_unreachable" } })
    );
    await expect(meshReadiness("dev-1")).resolves.toEqual({ error: "broker_unreachable" });
  });

  it("maps a failure that never reached HTTP to mesh_unavailable", async () => {
    fetchMock.mockRejectedValue(new TypeError("fetch failed"));
    await expect(meshReadiness("dev-1")).resolves.toEqual({ error: "mesh_unavailable" });
  });

  it("maps the edge's non-JSON answer to mesh_unavailable", async () => {
    fetchMock.mockResolvedValue(answer({ text: "<html>Access login</html>" }));
    await expect(meshReadiness("dev-1")).resolves.toEqual({ error: "mesh_unavailable" });
  });
});
