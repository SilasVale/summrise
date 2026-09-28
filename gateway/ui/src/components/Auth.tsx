import { useState, type FormEvent } from "react";
import { useAuth } from "../contexts/AuthContext.tsx";
import { useTranslation } from "../i18n.ts";
import { ApiError } from "../api/client.ts";

type Tab = "login" | "register" | "reset";

export default function Auth() {
  const { login, register, resetPassword } = useAuth();
  const { t, lang, setLang } = useTranslation();
  const [tab, setTab] = useState<Tab>("login");

  // Form fields
  const [loginUser, setLoginUser] = useState("");
  const [loginPass, setLoginPass] = useState("");
  const [regUser, setRegUser] = useState("");
  const [regPass, setRegPass] = useState("");
  const [regInvite, setRegInvite] = useState("");
  const [resetKey, setResetKey] = useState("");
  const [resetNewPass, setResetNewPass] = useState("");

  // Messages
  const [loginMsg, setLoginMsg] = useState("");
  const [regMsg, setRegMsg] = useState("");
  const [resetMsg, setResetMsg] = useState("");

  async function handleLogin(e: FormEvent) {
    e.preventDefault();
    setLoginMsg("");
    try {
      await login(loginUser, loginPass);
    } catch (err) {
      setLoginMsg(err instanceof ApiError ? err.message : t("auth.loginFail"));
    }
  }

  async function handleRegister(e: FormEvent) {
    e.preventDefault();
    setRegMsg("");
    try {
      await register(regUser, regPass, regInvite);
    } catch (err) {
      setRegMsg(err instanceof ApiError ? err.message : t("auth.registerFail"));
    }
  }

  async function handleReset(e: FormEvent) {
    e.preventDefault();
    setResetMsg("");
    try {
      await resetPassword(resetKey, resetNewPass);
      setTab("login");
      setLoginMsg(t("auth.resetOk"));
    } catch (err) {
      setResetMsg(err instanceof ApiError ? err.message : t("auth.resetFail"));
    }
  }

  return (
    <div className="auth-wrap">
      {/* The brand lives BESIDE the form, not stacked on top of it. The page was
          a 396px card floating in a 1440px field of nothing, with the only
          explanatory sentence — `auth.foot` — orphaned UNDER the card in the
          smallest type on the screen. Moving the brand out gives the page a
          composition and gives that sentence somewhere it can be read. */}
      <aside className="auth-aside">
        <div className="auth-lang">
          <button className="lang-btn" onClick={() => setLang(lang === "zh" ? "en" : "zh")}>
            {lang === "zh" ? "EN" : "中文"}{" "}
            {/* i18n-allow-cjk: names the other language in its own script */}
          </button>
        </div>
        <div className="auth-brand">
          <img className="brand-img" src="/favicon.svg" alt="" width={52} height={52} />
          <div>
            <h1>Summrise</h1>
            <p>{t("app.sub")}</p>
          </div>
        </div>
        <p className="auth-pitch">{t("auth.foot")}</p>
      </aside>

      <main className="auth-main">
        <div className="auth-card">
          <div className="auth-tabs" role="tablist">
            <button
              className={`auth-tab ${tab === "login" ? "active" : ""}`}
              onClick={() => setTab("login")}
            >
              {t("auth.login")}
            </button>
            <button
              className={`auth-tab ${tab === "register" ? "active" : ""}`}
              onClick={() => setTab("register")}
            >
              {t("auth.register")}
            </button>
            <button
              className={`auth-tab ${tab === "reset" ? "active" : ""}`}
              onClick={() => setTab("reset")}
            >
              {t("auth.resetTab")}
            </button>
          </div>

          {tab === "login" && (
            /* NO PLACEHOLDERS ON THIS FORM (2026-09-28). Measured on the live page: the label above
               each field is a VISIBLE 330x69.1 box whose text renders 26.1px above the input, and
               the field's accessible name is computed FROM that wrapping label — and `auth.usernamePh`
               was the SAME STRING as `auth.username`, `auth.passwordPh` the same string as
               `auth.password`. So the field's name was printed twice: once above the box by the
               label, once inside it by the placeholder.
               The test that decided WHICH of the two goes, answered both ways: delete the placeholder
               and the visible label still names the field (nothing is lost, and the name survives
               typing); delete the label and the name disappears the moment the user types (everything
               is lost). Hence this form, and only the two fields whose placeholder was an exact
               duplicate of its label.
               THE OTHER TWO TABS KEEP THEIRS, because there the placeholder carries a rule the label
               does not — `auth.usernamePhReg`, `auth.passwordPhReg`, `auth.newPasswordPh` state a
               length/charset constraint and `auth.invitePh` states a prerequisite — and a constraint
               that lives only in a placeholder is lost exactly when the user obeys it. Promoting those
               into a `.muted` paragraph (the shape `auth.resetHint` already uses on the reset tab) is
               the follow-up; it costs 54px per form and was left for its own commit.
               (Quoted by KEY rather than by value: this file may not carry CJK literals.) */
            <form className="auth-form" onSubmit={handleLogin} autoComplete="off">
              <label>
                <span>{t("auth.username")}</span>
                <input
                  name="username"
                  required
                  autoComplete="username"
                  value={loginUser}
                  onChange={(e) => setLoginUser(e.target.value)}
                />
              </label>
              <label>
                <span>{t("auth.password")}</span>
                <input
                  name="password"
                  type="password"
                  required
                  autoComplete="current-password"
                  value={loginPass}
                  onChange={(e) => setLoginPass(e.target.value)}
                />
              </label>
              {loginMsg && <p className="form-msg">{loginMsg}</p>}
              <button type="submit" className="btn btn-accent btn-block">
                {t("auth.loginBtn")}
              </button>
            </form>
          )}

          {tab === "register" && (
            <form className="auth-form" onSubmit={handleRegister} autoComplete="off">
              <label>
                <span>{t("auth.username")}</span>
                <input
                  name="username"
                  placeholder={t("auth.usernamePhReg")}
                  required
                  autoComplete="username"
                  value={regUser}
                  onChange={(e) => setRegUser(e.target.value)}
                />
              </label>
              <label>
                <span>{t("auth.password")}</span>
                <input
                  name="password"
                  type="password"
                  placeholder={t("auth.passwordPhReg")}
                  required
                  autoComplete="new-password"
                  value={regPass}
                  onChange={(e) => setRegPass(e.target.value)}
                />
              </label>
              <label>
                <span>{t("auth.inviteCode")}</span>
                <input
                  name="inviteCode"
                  placeholder={t("auth.invitePh")}
                  required
                  value={regInvite}
                  onChange={(e) => setRegInvite(e.target.value)}
                />
              </label>
              {regMsg && <p className="form-msg">{regMsg}</p>}
              <button type="submit" className="btn btn-accent btn-block">
                {t("auth.registerBtn")}
              </button>
            </form>
          )}

          {tab === "reset" && (
            <form className="auth-form" onSubmit={handleReset} autoComplete="off">
              <p className="muted">{t("auth.resetHint")}</p>
              <label>
                <span>{t("auth.adminKey")}</span>
                {/* THIS FIELD WAS NAMED TWO DIFFERENT THINGS, which is a contradiction rather than a
                    duplicate: the label above it renders `auth.adminKey` while the placeholder inside
                    it rendered `auth.adminKeyPh`, and the two dictionary values disagree — the label
                    and the `auth.resetHint` paragraph above the form say one term, the placeholder
                    said another. Two claims about one input, and two of the three agreed.
                    The label wins, because the label is the one that persists. The placeholder is
                    deleted rather than reworded to match: a reworded one would have left the field's
                    name printed twice, which is the defect the login form above was just cured of. */}
                <input
                  name="adminKey"
                  type="password"
                  required
                  autoComplete="off"
                  value={resetKey}
                  onChange={(e) => setResetKey(e.target.value)}
                />
              </label>
              <label>
                <span>{t("auth.newPassword")}</span>
                <input
                  name="newPassword"
                  type="password"
                  placeholder={t("auth.newPasswordPh")}
                  required
                  autoComplete="new-password"
                  value={resetNewPass}
                  onChange={(e) => setResetNewPass(e.target.value)}
                />
              </label>
              {resetMsg && <p className="form-msg">{resetMsg}</p>}
              <button type="submit" className="btn btn-accent btn-block">
                {t("auth.resetBtn")}
              </button>
            </form>
          )}
        </div>
      </main>
    </div>
  );
}
