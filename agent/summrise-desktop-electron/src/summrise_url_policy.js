/* @ts-self-types="./summrise_url_policy.d.ts" */

/**
 * The one wrapper with a decision of its own: the JavaScript threw a real `Error`, so a thrown STRING
 * would not be the same refusal (a caller printing `err.message` or matching a regex sees the
 * difference). The message is the TypeScript's, `${port}` included — which is why a non-number is
 * rendered with `String(value)` rather than dropped: `addDshPort("abc")` must say `not a port: abc`.
 * @param {number} port
 */
function addDshPort(port) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.addDshPort(retptr, addHeapObject(port));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        if (r1) {
            throw takeObject(r0);
        }
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
exports.addDshPort = addDshPort;

/**
 * @returns {string}
 */
function agentBase() {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.agentBase(retptr);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
    }
}
exports.agentBase = agentBase;

/**
 * @param {string} url
 * @returns {boolean}
 */
function certBypassAllowed(url) {
    const ret = wasm.certBypassAllowed(addHeapObject(url));
    return ret !== 0;
}
exports.certBypassAllowed = certBypassAllowed;

function clearExtraDshPorts() {
    wasm.clearExtraDshPorts();
}
exports.clearExtraDshPorts = clearExtraDshPorts;

/**
 * @param {string | null} [origin]
 * @returns {boolean}
 */
function controlOriginOk(origin) {
    const ret = wasm.controlOriginOk(isLikeNone(origin) ? 0 : addHeapObject(origin));
    return ret !== 0;
}
exports.controlOriginOk = controlOriginOk;

/**
 * @returns {string}
 */
function dshBase() {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.dshBase(retptr);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
    }
}
exports.dshBase = dshBase;

/**
 * @returns {string[]}
 */
function dshOrigins() {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.dshOrigins(retptr);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        var v1 = getArrayJsValueFromWasm0(r0, r1);
        wasm.__wbindgen_export3(r0, r1 * 4, 4);
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
exports.dshOrigins = dshOrigins;

/**
 * @param {string} url
 * @returns {boolean}
 */
function frameUrlOk(url) {
    const ret = wasm.frameUrlOk(addHeapObject(url));
    return ret !== 0;
}
exports.frameUrlOk = frameUrlOk;

/**
 * @returns {number}
 */
function getAgentPort() {
    const ret = wasm.getAgentPort();
    return ret;
}
exports.getAgentPort = getAgentPort;

/**
 * @returns {number}
 */
function getDshPort() {
    const ret = wasm.getDshPort();
    return ret;
}
exports.getDshPort = getDshPort;

/**
 * @param {string} url
 * @returns {boolean}
 */
function isBaseOrigin(url) {
    const ret = wasm.isBaseOrigin(addHeapObject(url));
    return ret !== 0;
}
exports.isBaseOrigin = isBaseOrigin;

/**
 * @param {string} url
 * @returns {boolean}
 */
function isDesktopSpaUrl(url) {
    const ret = wasm.isDesktopSpaUrl(addHeapObject(url));
    return ret !== 0;
}
exports.isDesktopSpaUrl = isDesktopSpaUrl;

/**
 * @param {string} url
 * @returns {boolean}
 */
function isDshUrl(url) {
    const ret = wasm.isDshUrl(addHeapObject(url));
    return ret !== 0;
}
exports.isDshUrl = isDshUrl;

/**
 * @param {string} hostname
 * @returns {boolean}
 */
function isPrivateHost(hostname) {
    const ret = wasm.isPrivateHost(addHeapObject(hostname));
    return ret !== 0;
}
exports.isPrivateHost = isPrivateHost;

/**
 * `Option<u16>` rather than a `JsValue` holding `null`: the generated `.d.ts` then says
 * `number | undefined` instead of `any`, and the shell's one call site is `if (port) return port;` — a
 * truthiness test, which `undefined` and `null` answer the same way. The TypeScript answered `null`;
 * this is the one place the JS-visible answer is spelled differently, and nothing can observe it.
 * @param {string} yaml_text
 * @returns {number | undefined}
 */
function parseAgentPort(yaml_text) {
    const ret = wasm.parseAgentPort(addHeapObject(yaml_text));
    return ret === 0xFFFFFF ? undefined : ret;
}
exports.parseAgentPort = parseAgentPort;

/**
 * @param {string | null} [url]
 * @returns {string}
 */
function sanitizeBrowserUrl(url) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.sanitizeBrowserUrl(retptr, isLikeNone(url) ? 0 : addHeapObject(url));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export3(deferred1_0, deferred1_1, 1);
    }
}
exports.sanitizeBrowserUrl = sanitizeBrowserUrl;

/**
 * @param {number} port
 */
function setAgentPort(port) {
    wasm.setAgentPort(addHeapObject(port));
}
exports.setAgentPort = setAgentPort;

/**
 * @param {number} port
 */
function setDshPort(port) {
    wasm.setDshPort(addHeapObject(port));
}
exports.setDshPort = setDshPort;
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg_String_73fabd11edc2c89f: function(arg0) {
            const ret = String(getObject(arg0));
            return addHeapObject(ret);
        },
        __wbg___wbindgen_is_falsy_16bd49b68658263e: function(arg0) {
            const ret = !getObject(arg0);
            return ret;
        },
        __wbg___wbindgen_is_undefined_8865fb403f8fe9d8: function(arg0) {
            const ret = getObject(arg0) === undefined;
            return ret;
        },
        __wbg___wbindgen_number_get_2e0e7dee9f701a71: function(arg0, arg1) {
            const obj = getObject(arg1);
            const ret = typeof(obj) === 'number' ? obj : undefined;
            getDataViewMemory0().setFloat64(arg0 + 8 * 1, isLikeNone(ret) ? 0 : ret, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, !isLikeNone(ret), true);
        },
        __wbg___wbindgen_string_get_0380ccaa2f57f0d9: function(arg0, arg1) {
            const obj = getObject(arg1);
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_export, wasm.__wbindgen_export2);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_41e9ee4f547fc59a: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_new_343a093a3c2ffb4e: function(arg0, arg1) {
            const ret = new Error(getStringFromWasm0(arg0, arg1));
            return addHeapObject(ret);
        },
        __wbindgen_generic_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return addHeapObject(ret);
        },
        __wbindgen_object_drop_ref: function(arg0) {
            takeObject(arg0);
        },
    };
    return {
        __proto__: null,
        "./summrise_url_policy_bg.js": import0,
    };
}

function addHeapObject(obj) {
    if (heap_next === heap.length) heap.push(heap.length + 1);
    const idx = heap_next;
    heap_next = heap[idx];

    heap[idx] = obj;
    return idx;
}

function dropObject(idx) {
    if (idx < 1028) return;
    heap[idx] = heap_next;
    heap_next = idx;
}

function getArrayJsValueFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    const mem = getDataViewMemory0();
    const result = [];
    for (let i = ptr; i < ptr + 4 * len; i += 4) {
        result.push(takeObject(mem.getUint32(i, true)));
    }
    return result;
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

function getObject(idx) { return heap[idx]; }

let heap = new Array(1024).fill(undefined);
heap.push(undefined, null, true, false);

let heap_next = heap.length;

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

function takeObject(idx) {
    const ret = getObject(idx);
    dropObject(idx);
    return ret;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
function decodeText(ptr, len) {
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

const wasmPath = `${__dirname}/summrise_url_policy_bg.wasm`;
const wasmBytes = require('fs').readFileSync(wasmPath);
const wasmModule = new WebAssembly.Module(wasmBytes);
let wasmInstance = new WebAssembly.Instance(wasmModule, __wbg_get_imports());
let wasm = wasmInstance.exports;
