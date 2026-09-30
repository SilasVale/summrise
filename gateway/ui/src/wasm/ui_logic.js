/* @ts-self-types="./ui_logic.d.ts" */

/**
 * `agentSignal(status, t)` — the agent's row: what it is called, which of the three states, and the
 * word for it.
 *
 * `signalOf(status?.agent_up, …)`: an ABSENT status makes the field `undefined`, which is the
 * `off` / "not checked" arm. The field's TYPE is `boolean | undefined`, so the JS test is
 * `value === undefined` — distinct from `false`, which is `err`. `js_sys::JsValue` has no
 * "is undefined" in the loose sense, so the test is `is_undefined()` here.
 * @param {any} status
 * @param {Function} t
 * @returns {object}
 */
export function agent_signal(status, t) {
    const ret = wasm.agent_signal(status, t);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * THE BARE PREFIX (`or/` and `or` are one channel).
 *
 * `String(prefix ?? "")` — the JavaScript coerces ANY value (a number, `null`, `undefined`) rather
 * than only accepting a string, so the input stays a `JsValue` and the coercion is the ENGINE's
 * `ToString`, not Rust's formatter. That is this migration's standing rule: ask the engine for a
 * value's text, never `format!` — a `key` spelled `1e+21` on one side and
 * `1000000000000000000000` on the other is a different key.
 *
 * **AND THIS FUNCTION CLAIMED THAT RULE WHILE BREAKING IT (found 2026-09-29, by the test that wired
 * it).** The body was `JsString::from(prefix)`, which is an UPCAST and not a coercion: it reinterprets
 * the value as a string without converting it, so `as_string()` answers `None` for a number, a `null`
 * and an `undefined`, and every one of them became `""`. The TypeScript's `String(5)` is `"5"`. The
 * console's `test/lane.test.mjs` pins exactly that case, which is why it was found the moment the
 * function was reachable rather than the day a caller sent a number.
 *
 * The `?? ""` half is `is_null() || is_undefined()`: `String(null)` is `"null"`, and the TypeScript
 * asked for the empty string.
 * @param {any} prefix
 * @returns {string}
 */
export function bare_prefix(prefix) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.bare_prefix(prefix);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * The words for that signal, given the provider's reason when it has one.
 *
 * `c.ok ? t("overview.healthOk") : c.reason || t("overview.healthDown")` — the `reason` wins over the
 * generic line, because a provider that says WHY is more useful than a label that says WHAT. The
 * `||` is a TRUTHINESS test on the reason as well: an empty string is no reason, and the TypeScript
 * falls through to the generic line for it.
 *
 * **AND IT RETURNS THE REASON AS IT ARRIVED, WHICH IS NOT A STRING AND IS THE TYPESCRIPT'S OWN
 * BEHAVIOUR.** `c.reason || t(…)` returns the VALUE: the first version of this port stringified it
 * through the engine and answered `"[object Object]"` where the JavaScript answers `{}`. The return
 * type has always said `string` while the body could hand back anything truthy, and the differential
 * is what turned that into a fact — so the port is faithful and the cast stays in the wrapper.
 * @param {any} channel
 * @param {Function} t
 * @returns {any}
 */
export function channel_label(channel, t) {
    const ret = wasm.channel_label(channel, t);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * The signal for a channel: `ok` is the provider's own answer, anything else is a failure with a
 * reason the row shows.
 *
 * **TRUTHINESS, NOT `=== true`** — the TypeScript is `ok ? "ok" : "err"`, so a `1` is `ok` there and
 * `false` here would be a divergence on the same input.
 * @param {any} ok
 * @returns {string}
 */
export function channel_signal(ok) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.channel_signal(ok);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * `UPDATE_DARK_WINDOW_MS`, for the one caller that needs the NUMBER rather than the rule:
 * `rememberedUpdateAttempt`'s store filter, which asks here so that the console keeps ONE definition
 * of the window. A FUNCTION and not a module-level constant, because a module-level `logic()` call
 * evaluates before any test can await the seam — the mistake the note below records for
 * `CONSOLE_POLL_MS`.
 * @returns {number}
 */
export function dark_window_ms() {
    const ret = wasm.dark_window_ms();
    return ret;
}

/**
 * `deviceIsUp(status)` — `!!status?.agent_up`, i.e. TRUTHINESS on the optional chain. A caller that
 * needs "offline" rather than "not checked" reads the signal below instead.
 * @param {any} status
 * @returns {boolean}
 */
export function device_is_up(status) {
    const ret = wasm.device_is_up(status);
    return ret !== 0;
}

/**
 * `deviceTally(devices, statuses)` — THE COUNTS, IN ONE PLACE.
 *
 * Two surfaces used to compute "N devices online" from the same fact with their own
 * `filter(…).length`, which is how the per-device mark disagreed with the count. A count cannot show
 * ambiguity per device, so BOTH numbers are returned: `online` is what is known, `unchecked` is what
 * is not yet known. `devices ?? []` — a nullish list is an EMPTY list, not a throw.
 * @param {any} devices
 * @param {any} statuses
 * @returns {object}
 */
export function device_tally(devices, statuses) {
    const ret = wasm.device_tally(devices, statuses);
    return ret;
}

/**
 * A DIAL'S TONE, from "how many of N are well" — the same question the channels tile and the devices
 * tile both ask, and they used to answer it differently.
 *
 * `known` is separate from `total` because "we have not asked yet" is not "none are healthy" — the
 * same distinction `deviceState.ts` records for a probe has not answered.
 *
 * **BOTH COUNTS ARRIVE AS VALUES, NOT NUMBERS, AND THAT IS THE SECOND DIVERGENCE THE DIFFERENTIAL
 * FOUND.** `ok === total` is JavaScript's STRICT equality and `ok > 0` is a RELATIONAL comparison,
 * and they disagree on everything that is not a number: for `ok = true, total = 1` the first is
 * `false` and the second is `true`, so the TypeScript answers `warn` — and a port that took two
 * `f64` parameters turned that into `1 === 1` and answered `ok`. Reading the two through the
 * operators the TypeScript actually wrote is the whole fix; see `strictly_equals` and `to_number`.
 * @param {any} known
 * @param {any} ok
 * @param {any} total
 * @returns {string}
 */
export function health_tone(known, ok, total) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.health_tone(known, ok, total);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * The lane class for a channel prefix, with its trailing slash ignored.
 * @param {any} prefix
 * @returns {string}
 */
export function lane_class(prefix) {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.lane_class(prefix);
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * `tunnelKnownDown(status)` — THE TRI-STATE RULE, IN ONE PLACE.
 *
 * `status?.tunnel_up === false` — STRICT equality against `false`, so an absent flag and an absent
 * status are both `false` here: "not known down" is not "known up". That distinction is the whole
 * point of the module; the differential carries `tunnel_up` missing, `true`, `false` and `null`.
 * @param {any} status
 * @returns {boolean}
 */
export function tunnel_known_down(status) {
    const ret = wasm.tunnel_known_down(status);
    return ret !== 0;
}

/**
 * `tunnelSignal(status, t)` — the same three states for the tunnel, because the gateway may not
 * have probed it either.
 * @param {any} status
 * @param {Function} t
 * @returns {object}
 */
export function tunnel_signal(status, t) {
    const ret = wasm.tunnel_signal(status, t);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * `updateControl({ update, agentSignal, remembered, now })` — the one function every render of this
 * control goes through.
 *
 * THE ARMS, IN THE ORDER THE TYPESCRIPT TESTS THEM, because that order is the module:
 *
 *   update truthy   `busy`               -> inflight, dated by `datableStart`, source `device`
 *                   available + `latest` -> action (a non-empty STRING `latest`, not just a truthy one)
 *                   `error` (non-empty)  -> unchecked — it OUTRANKS the pin, because "we could not
 *                                           ask" is not "it is held": a pin claim needs a channel
 *                   `pinned_to`          -> held
 *                   otherwise            -> current
 *   update absent   `agentSignal === "err"` AND this console's own record, still inside the window
 *                                        -> inflight, source `console`
 *                   otherwise            -> none, which is the absence of a sentence
 *
 * `agentSignal === "err"` IS NOT `!agentUp`, and that is the defect this module exists to remove: a
 * device nobody has asked about yet is ALSO falsy, so a boolean test would speak about every row of a
 * console before its first poll answered. `now` defaults with `?? Date.now()`, so `null` and `absent`
 * both mean "the engine's clock" and a `0` is a real (and unusable) instant.
 * @param {any} update
 * @param {any} agent_signal
 * @param {any} remembered
 * @param {any} now
 * @returns {any}
 */
export function update_control(update, agent_signal, remembered, now) {
    const ret = wasm.update_control(update, agent_signal, remembered, now);
    return ret;
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_is_falsy_16bd49b68658263e: function(arg0) {
            const ret = !arg0;
            return ret;
        },
        __wbg___wbindgen_is_function_1f9d30630b8b1d3d: function(arg0) {
            const ret = typeof(arg0) === 'function';
            return ret;
        },
        __wbg___wbindgen_is_null_e343b7d08827ba72: function(arg0) {
            const ret = arg0 === null;
            return ret;
        },
        __wbg___wbindgen_is_undefined_8865fb403f8fe9d8: function(arg0) {
            const ret = arg0 === undefined;
            return ret;
        },
        __wbg___wbindgen_number_get_2e0e7dee9f701a71: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'number' ? obj : undefined;
            getDataViewMemory0().setFloat64(arg0 + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_string_get_0380ccaa2f57f0d9: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_41e9ee4f547fc59a: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg___wbindgen_typeof_e777a26e115d416b: function(arg0) {
            const ret = typeof arg0;
            return ret;
        },
        __wbg_call_187d372bd5fdd4aa: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.call(arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_from_296ca31f8d0f1c52: function(arg0) {
            const ret = Array.from(arg0);
            return ret;
        },
        __wbg_get_31af05bd4842a84f: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_get_unchecked_288889d017702237: function(arg0, arg1) {
            const ret = arg0[arg1 >>> 0];
            return ret;
        },
        __wbg_isArray_2b41c29f43a3fb12: function(arg0) {
            const ret = Array.isArray(arg0);
            return ret;
        },
        __wbg_length_d4bdea10311bd9cf: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_new_617a8cdb8bb1130e: function() {
            const ret = new Object();
            return ret;
        },
        __wbg_now_aa4ccb83129e9e55: function() {
            const ret = Date.now();
            return ret;
        },
        __wbg_set_145a351398b48c65: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = Reflect.set(arg0, arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_static_accessor_GLOBAL_266715b9d96ba635: function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_GLOBAL_THIS_10fb7dc1ae063179: function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_SELF_0b583911f537483a: function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_WINDOW_d7f903d1508cbdc4: function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbindgen_generic_0000000000000001: function(arg0) {
            // Cast intrinsic for `F64 -> Externref`.
            const ret = arg0;
            return ret;
        },
        __wbindgen_generic_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./ui_logic_bg.js": import0,
    };
}

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (!module.ok) {
            throw new Error(`failed to fetch Wasm: ${module.status} ${module.statusText} fetching '${module.url}'`);
        }

        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('ui_logic_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
