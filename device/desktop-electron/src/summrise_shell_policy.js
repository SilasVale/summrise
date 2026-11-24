/* @ts-self-types="./summrise_shell_policy.d.ts" */

/**
 * @param {string} agent_base
 * @returns {string}
 */
function agentHostLabel(agent_base) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.agentHostLabel(retptr, addHeapObject(agent_base));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.agentHostLabel = agentHostLabel;

/**
 * @param {string} platform
 * @param {string} app_name
 * @returns {string}
 */
function appMenuJson(platform, app_name) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.appMenuJson(retptr, addHeapObject(platform), addHeapObject(app_name));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.appMenuJson = appMenuJson;

/**
 * @param {string} platform
 * @returns {string}
 */
function aumidReport(platform) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.aumidReport(retptr, addHeapObject(platform));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.aumidReport = aumidReport;

/**
 * @param {string | null} [token]
 * @returns {string | undefined}
 */
function authorization(token) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.authorization(retptr, isLikeNone(token) ? 0 : addHeapObject(token));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        let v1;
        if (r0 !== 0) {
            v1 = getStringFromWasm0(r0, r1);
            wasm.__wbindgen_export4(r0, r1 * 1, 1);
        }
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
exports.authorization = authorization;

/**
 * @param {boolean} enabled
 * @param {boolean} exists
 * @returns {string}
 */
function autoLaunchPlan(enabled, exists) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.autoLaunchPlan(retptr, enabled, exists);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.autoLaunchPlan = autoLaunchPlan;

/**
 * @param {number} now_ms
 * @returns {string}
 */
function browserId(now_ms) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.browserId(retptr, now_ms);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.browserId = browserId;

/**
 * @param {number} port
 * @returns {string}
 */
function cdpEndpoint(port) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.cdpEndpoint(retptr, port);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.cdpEndpoint = cdpEndpoint;

/**
 * @param {number} port
 * @returns {string}
 */
function cdpSelfCheckOk(port) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.cdpSelfCheckOk(retptr, port);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.cdpSelfCheckOk = cdpSelfCheckOk;

/**
 * @param {string} version_json
 * @returns {boolean}
 */
function cdpSelfCheckOwnsPort(version_json) {
    const ret = wasm.cdpSelfCheckOwnsPort(addHeapObject(version_json));
    return ret !== 0;
}
exports.cdpSelfCheckOwnsPort = cdpSelfCheckOwnsPort;

/**
 * @param {string} version_json
 * @returns {string}
 */
function cdpUserAgent(version_json) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.cdpUserAgent(retptr, addHeapObject(version_json));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.cdpUserAgent = cdpUserAgent;

/**
 * @param {string | null} [user_agent]
 * @returns {boolean}
 */
function cdpUserAgentOurs(user_agent) {
    const ret = wasm.cdpUserAgentOurs(isLikeNone(user_agent) ? 0 : addHeapObject(user_agent));
    return ret !== 0;
}
exports.cdpUserAgentOurs = cdpUserAgentOurs;

/**
 * @param {number} port
 * @param {string} user_agent
 * @returns {string}
 */
function cdpWarningForeign(port, user_agent) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.cdpWarningForeign(retptr, port, addHeapObject(user_agent));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.cdpWarningForeign = cdpWarningForeign;

/**
 * @param {number} port
 * @returns {string}
 */
function cdpWarningNotResponding(port) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.cdpWarningNotResponding(retptr, port);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.cdpWarningNotResponding = cdpWarningNotResponding;

/**
 * @param {number} port
 * @returns {string}
 */
function cdpWarningUnreachable(port) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.cdpWarningUnreachable(retptr, port);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.cdpWarningUnreachable = cdpWarningUnreachable;

/**
 * @param {string} method
 * @returns {boolean}
 */
function controlIsPreflight(method) {
    const ret = wasm.controlIsPreflight(addHeapObject(method));
    return ret !== 0;
}
exports.controlIsPreflight = controlIsPreflight;

/**
 * @param {string | null} [raw_url]
 * @returns {string | undefined}
 */
function controlPath(raw_url) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.controlPath(retptr, isLikeNone(raw_url) ? 0 : addHeapObject(raw_url));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        let v1;
        if (r0 !== 0) {
            v1 = getStringFromWasm0(r0, r1);
            wasm.__wbindgen_export4(r0, r1 * 1, 1);
        }
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
exports.controlPath = controlPath;

/**
 * @param {string | null | undefined} raw_url
 * @param {string} key
 * @returns {string | undefined}
 */
function controlQuery(raw_url, key) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.controlQuery(retptr, addHeapObject(raw_url), addHeapObject(key));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        let v1;
        if (r0 !== 0) {
            v1 = getStringFromWasm0(r0, r1);
            wasm.__wbindgen_export4(r0, r1 * 1, 1);
        }
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
exports.controlQuery = controlQuery;

/**
 * @param {string} method
 * @param {string} pathname
 * @returns {string}
 */
function controlRoute(method, pathname) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.controlRoute(retptr, addHeapObject(method), addHeapObject(pathname));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.controlRoute = controlRoute;

/**
 * @param {string | null} [origin]
 * @returns {string}
 */
function corsAllowOrigin(origin) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.corsAllowOrigin(retptr, isLikeNone(origin) ? 0 : addHeapObject(origin));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.corsAllowOrigin = corsAllowOrigin;

/**
 * @param {string | null} [config_yaml]
 * @returns {string | undefined}
 */
function deviceToken(config_yaml) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.deviceToken(retptr, isLikeNone(config_yaml) ? 0 : addHeapObject(config_yaml));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        let v1;
        if (r0 !== 0) {
            v1 = getStringFromWasm0(r0, r1);
            wasm.__wbindgen_export4(r0, r1 * 1, 1);
        }
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
exports.deviceToken = deviceToken;

/**
 * @param {string} dsh_base
 * @returns {string}
 */
function dshHome(dsh_base) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.dshHome(retptr, addHeapObject(dsh_base));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.dshHome = dshHome;

/**
 * `admitted` is `isDshUrl(raw)`, evaluated by the host.
 * @param {string} raw
 * @param {boolean} admitted
 * @returns {string | undefined}
 */
function dshPopupTarget(raw, admitted) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.dshPopupTarget(retptr, addHeapObject(raw), admitted);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        let v1;
        if (r0 !== 0) {
            v1 = getStringFromWasm0(r0, r1);
            wasm.__wbindgen_export4(r0, r1 * 1, 1);
        }
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
exports.dshPopupTarget = dshPopupTarget;

/**
 * `admitted` is `isDshUrl(raw)`, evaluated by the host.
 * @param {string} raw
 * @param {boolean} admitted
 * @returns {string}
 */
function dshTarget(raw, admitted) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.dshTarget(retptr, addHeapObject(raw), admitted);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.dshTarget = dshTarget;

/**
 * The DROP rule for a popup the load door refused. The argument is the DECIDED target.
 * @param {string} target
 * @returns {string | undefined}
 */
function embeddedPopupTarget(target) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.embeddedPopupTarget(retptr, addHeapObject(target));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        let v1;
        if (r0 !== 0) {
            v1 = getStringFromWasm0(r0, r1);
            wasm.__wbindgen_export4(r0, r1 * 1, 1);
        }
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
exports.embeddedPopupTarget = embeddedPopupTarget;

/**
 * @param {string | null} [last]
 * @returns {string}
 */
function embeddedRecoverUrl(last) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.embeddedRecoverUrl(retptr, isLikeNone(last) ? 0 : addHeapObject(last));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.embeddedRecoverUrl = embeddedRecoverUrl;

/**
 * `new Date(at).toTimeString().slice(0, 8)` — with the host's time-zone offset passed in, because wasm
 * has no clock and no zone database.
 * @param {number} at_ms
 * @param {number} tz_offset_min
 * @returns {string}
 */
function fmtClock(at_ms, tz_offset_min) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.fmtClock(retptr, at_ms, tz_offset_min);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.fmtClock = fmtClock;

/**
 * @param {number} secs
 * @returns {string}
 */
function fmtUptime(secs) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.fmtUptime(retptr, secs);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.fmtUptime = fmtUptime;

/**
 * @returns {string}
 */
function forbiddenFrame() {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.forbiddenFrame(retptr);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.forbiddenFrame = forbiddenFrame;

/**
 * @param {number} delta
 * @returns {boolean}
 */
function goBackwards(delta) {
    const ret = wasm.goBackwards(delta);
    return ret !== 0;
}
exports.goBackwards = goBackwards;

/**
 * THE ADMISSIBLE HARNESS DOORS, in the order the answer named them.
 *
 * The argument is the ALREADY-PARSED `answer.harnesses` array, because `Array.isArray` and
 * `row?.local_port` are JavaScript operations on a JavaScript value; what is decided here is which
 * numbers pass the range test. The host then calls `addDshPort` — the url-policy crate's — for each,
 * because admitting a door is that crate's decision over its own list and this one must not hold a
 * second copy of it (see `Cargo.toml`). So the shell's harness table is: **the agent names the ports,
 * this crate says which are usable, and the url policy says which doors exist.**
 * @param {any} rows
 * @returns {Uint16Array}
 */
function harnessDoors(rows) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.harnessDoors(retptr, addHeapObject(rows));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        var v1 = getArrayU16FromWasm0(r0, r1).slice();
        wasm.__wbindgen_export4(r0, r1 * 2, 2);
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
exports.harnessDoors = harnessDoors;

/**
 * @param {string} url
 * @returns {boolean}
 */
function isWaitPage(url) {
    const ret = wasm.isWaitPage(addHeapObject(url));
    return ret !== 0;
}
exports.isWaitPage = isWaitPage;

/**
 * @param {number} current
 * @returns {number}
 */
function nextRetryMs(current) {
    const ret = wasm.nextRetryMs(current);
    return ret;
}
exports.nextRetryMs = nextRetryMs;

/**
 * `browserOpen`'s plan: `{"reuse": id|null, "evict": id|null}`.
 * @param {string} existing_json
 * @param {string} target
 * @param {number} cap
 * @returns {string}
 */
function planBrowserOpen(existing_json, target, cap) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.planBrowserOpen(retptr, addHeapObject(existing_json), addHeapObject(target), cap);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.planBrowserOpen = planBrowserOpen;

/**
 * ONE POLL'S WHOLE DECISION: the parse, the keep-last rule, the vitals line and the tooltip.
 * @param {string} request_json
 * @returns {string}
 */
function refreshTrayHealth(request_json) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.refreshTrayHealth(retptr, addHeapObject(request_json));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.refreshTrayHealth = refreshTrayHealth;

/**
 * @param {string | null} [env]
 * @param {number | undefined} [config_port]
 * @returns {number}
 */
function resolveAgentPort(env, config_port) {
    const ret = wasm.resolveAgentPort(isLikeNone(env) ? 0 : addHeapObject(env), !isLikeNone(config_port), isLikeNone(config_port) ? 0 : config_port);
    return ret;
}
exports.resolveAgentPort = resolveAgentPort;

/**
 * @param {string | null} [env]
 * @returns {number}
 */
function resolveDshPort(env) {
    const ret = wasm.resolveDshPort(isLikeNone(env) ? 0 : addHeapObject(env));
    return ret;
}
exports.resolveDshPort = resolveDshPort;

/**
 * THE ARGUMENT LIST WITH THE DOCUMENTED QUOTING TRAP IN IT — the `/tr` value carries its own inner
 * quotes, because `schtasks` re-parses it as a command line.
 * @param {string} task
 * @param {string} script
 * @returns {string[]}
 */
function schtasksCreateArgs(task, script) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.schtasksCreateArgs(retptr, addHeapObject(task), addHeapObject(script));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        var v1 = getArrayJsValueFromWasm0(r0, r1);
        wasm.__wbindgen_export4(r0, r1 * 4, 4);
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
exports.schtasksCreateArgs = schtasksCreateArgs;

/**
 * @param {string} task
 * @returns {string[]}
 */
function schtasksDeleteArgs(task) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.schtasksDeleteArgs(retptr, addHeapObject(task));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        var v1 = getArrayJsValueFromWasm0(r0, r1);
        wasm.__wbindgen_export4(r0, r1 * 4, 4);
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
exports.schtasksDeleteArgs = schtasksDeleteArgs;

/**
 * @param {string} task
 * @returns {string[]}
 */
function schtasksEndArgs(task) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.schtasksEndArgs(retptr, addHeapObject(task));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        var v1 = getArrayJsValueFromWasm0(r0, r1);
        wasm.__wbindgen_export4(r0, r1 * 4, 4);
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
exports.schtasksEndArgs = schtasksEndArgs;

/**
 * @param {string} task
 * @returns {string[]}
 */
function schtasksQueryArgs(task) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.schtasksQueryArgs(retptr, addHeapObject(task));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        var v1 = getArrayJsValueFromWasm0(r0, r1);
        wasm.__wbindgen_export4(r0, r1 * 4, 4);
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
exports.schtasksQueryArgs = schtasksQueryArgs;

/**
 * @param {string} task
 * @returns {string[]}
 */
function schtasksRunArgs(task) {
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.schtasksRunArgs(retptr, addHeapObject(task));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        var v1 = getArrayJsValueFromWasm0(r0, r1);
        wasm.__wbindgen_export4(r0, r1 * 4, 4);
        return v1;
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
    }
}
exports.schtasksRunArgs = schtasksRunArgs;

/**
 * @returns {string}
 */
function shellConstants() {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.shellConstants(retptr);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.shellConstants = shellConstants;

/**
 * @param {boolean} is_main_frame
 * @param {number} error_code
 * @returns {boolean}
 */
function shouldRetryLoad(is_main_frame, error_code) {
    const ret = wasm.shouldRetryLoad(is_main_frame, error_code);
    return ret !== 0;
}
exports.shouldRetryLoad = shouldRetryLoad;

/**
 * @param {string | null | undefined} live
 * @param {string} fallback
 * @returns {string}
 */
function shownUrl(live, fallback) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.shownUrl(retptr, addHeapObject(live), addHeapObject(fallback));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.shownUrl = shownUrl;

/**
 * `!bounds || bounds.width < 50 || bounds.height < 50`.
 * @param {any} bounds
 * @returns {boolean}
 */
function slotTooSmall(bounds) {
    const ret = wasm.slotTooSmall(addHeapObject(bounds));
    return ret !== 0;
}
exports.slotTooSmall = slotTooSmall;

/**
 * @param {number | null} [status_code]
 * @returns {boolean}
 */
function statusIsAlive(status_code) {
    const ret = wasm.statusIsAlive(!isLikeNone(status_code), isLikeNone(status_code) ? 0 : status_code);
    return ret !== 0;
}
exports.statusIsAlive = statusIsAlive;

/**
 * @param {number} cached_at
 * @param {number} now
 * @returns {boolean}
 */
function tokenCacheFresh(cached_at, now) {
    const ret = wasm.tokenCacheFresh(cached_at, now);
    return ret !== 0;
}
exports.tokenCacheFresh = tokenCacheFresh;

/**
 * @param {string} platform
 * @returns {string}
 */
function trayIconName(platform) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.trayIconName(retptr, addHeapObject(platform));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.trayIconName = trayIconName;

/**
 * `base_origin` is `isBaseOrigin(mainUrl)`, evaluated by the host — and only when the window is live,
 * which keeps the TypeScript's short-circuit.
 * @param {boolean} running
 * @param {boolean} watch_active
 * @param {boolean} window_live
 * @param {boolean} base_origin
 * @returns {boolean}
 */
function trayShouldWatch(running, watch_active, window_live, base_origin) {
    const ret = wasm.trayShouldWatch(running, watch_active, window_live, base_origin);
    return ret !== 0;
}
exports.trayShouldWatch = trayShouldWatch;

/**
 * `desktop_spa` is `isDesktopSpaUrl(url)`, evaluated by the host.
 * @param {string} url
 * @param {boolean} desktop_spa
 * @returns {boolean}
 */
function tripwireAllows(url, desktop_spa) {
    const ret = wasm.tripwireAllows(addHeapObject(url), desktop_spa);
    return ret !== 0;
}
exports.tripwireAllows = tripwireAllows;

/**
 * @param {string} url
 * @returns {string}
 */
function tripwireLog(url) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.tripwireLog(retptr, addHeapObject(url));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.tripwireLog = tripwireLog;

/**
 * @param {string} platform
 * @returns {boolean}
 */
function usesAppUserModelId(platform) {
    const ret = wasm.usesAppUserModelId(addHeapObject(platform));
    return ret !== 0;
}
exports.usesAppUserModelId = usesAppUserModelId;

/**
 * @param {boolean} want_visible
 * @param {boolean} live_contents
 * @returns {boolean}
 */
function viewVisible(want_visible, live_contents) {
    const ret = wasm.viewVisible(want_visible, live_contents);
    return ret !== 0;
}
exports.viewVisible = viewVisible;

/**
 * @param {boolean} ok
 * @param {string | null} [error]
 * @returns {string}
 */
function watchdogLog(ok, error) {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.watchdogLog(retptr, ok, isLikeNone(error) ? 0 : addHeapObject(error));
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.watchdogLog = watchdogLog;

/**
 * @param {number} misses
 * @param {number} last_start_at
 * @param {number} now
 * @returns {boolean}
 */
function watchdogShouldStart(misses, last_start_at, now) {
    const ret = wasm.watchdogShouldStart(misses, last_start_at, now);
    return ret !== 0;
}
exports.watchdogShouldStart = watchdogShouldStart;

/**
 * @returns {string}
 */
function windowIconName() {
    let deferred1_0;
    let deferred1_1;
    try {
        const retptr = wasm.__wbindgen_add_to_stack_pointer(-16);
        wasm.windowIconName(retptr);
        var r0 = getDataViewMemory0().getInt32(retptr + 4 * 0, true);
        var r1 = getDataViewMemory0().getInt32(retptr + 4 * 1, true);
        deferred1_0 = r0;
        deferred1_1 = r1;
        return getStringFromWasm0(r0, r1);
    } finally {
        wasm.__wbindgen_add_to_stack_pointer(16);
        wasm.__wbindgen_export4(deferred1_0, deferred1_1, 1);
    }
}
exports.windowIconName = windowIconName;

/**
 * `Math.min(3, Math.max(0.5, Number(factor) || 1))` — and the `|| 1` is applied HERE, on the value
 * `Number()` produced, because that is the order the JavaScript evaluated it in.
 * @param {any} factor
 * @returns {number}
 */
function zoomFactor(factor) {
    const ret = wasm.zoomFactor(addHeapObject(factor));
    return ret;
}
exports.zoomFactor = zoomFactor;
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg_Number_13c622b652a70404: function(arg0) {
            const ret = Number(getObject(arg0));
            return ret;
        },
        __wbg_String_a45bd2b9b9b48f70: function(arg0) {
            const ret = String(getObject(arg0));
            return addHeapObject(ret);
        },
        __wbg___wbindgen_is_falsy_16bd49b68658263e: function(arg0) {
            const ret = !getObject(arg0);
            return ret;
        },
        __wbg___wbindgen_is_object_3c45d4f2dde4e749: function(arg0) {
            const val = getObject(arg0);
            const ret = typeof(val) === 'object' && val !== null;
            return ret;
        },
        __wbg___wbindgen_is_undefined_8865fb403f8fe9d8: function(arg0) {
            const ret = getObject(arg0) === undefined;
            return ret;
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
        __wbg_from_296ca31f8d0f1c52: function(arg0) {
            const ret = Array.from(getObject(arg0));
            return addHeapObject(ret);
        },
        __wbg_get_31af05bd4842a84f: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(getObject(arg0), getObject(arg1));
            return addHeapObject(ret);
        }, arguments); },
        __wbg_get_unchecked_288889d017702237: function(arg0, arg1) {
            const ret = getObject(arg0)[arg1 >>> 0];
            return addHeapObject(ret);
        },
        __wbg_isArray_e15a2ff68ffdbef2: function(arg0) {
            const ret = Array.isArray(getObject(arg0));
            return ret;
        },
        __wbg_length_d4bdea10311bd9cf: function(arg0) {
            const ret = getObject(arg0).length;
            return ret;
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
        "./summrise_shell_policy_bg.js": import0,
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

function getArrayU16FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint16ArrayMemory0().subarray(ptr / 2, ptr / 2 + len);
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

let cachedUint16ArrayMemory0 = null;
function getUint16ArrayMemory0() {
    if (cachedUint16ArrayMemory0 === null || cachedUint16ArrayMemory0.byteLength === 0) {
        cachedUint16ArrayMemory0 = new Uint16Array(wasm.memory.buffer);
    }
    return cachedUint16ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function getObject(idx) { return heap[idx]; }

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        wasm.__wbindgen_export3(addHeapObject(e));
    }
}

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

const wasmPath = `${__dirname}/summrise_shell_policy_bg.wasm`;
const wasmBytes = require('fs').readFileSync(wasmPath);
const wasmModule = new WebAssembly.Module(wasmBytes);
let wasmInstance = new WebAssembly.Instance(wasmModule, __wbg_get_imports());
let wasm = wasmInstance.exports;
