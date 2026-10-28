import assert from "node:assert/strict";
import path from "node:path";
import { test } from "node:test";
import { browserRuntimeEnv, defaultUserDataDir, isDefaultProfile } from "./browser-profile.mjs";

const work = path.resolve("/tmp/smoke-work");
const linuxEnv = { HOME: "/tmp/home", XDG_CONFIG_HOME: "/tmp/home/.config" };

test("Linux default user-data directory follows CHROME_CONFIG_HOME, XDG_CONFIG_HOME, HOME", () => {
  assert.equal(
    defaultUserDataDir({ platform: "linux", kind: "chrome", env: { HOME: "/tmp/home" } }),
    "/tmp/home/.config/google-chrome",
  );
  assert.equal(
    defaultUserDataDir({ platform: "linux", kind: "edge", env: linuxEnv }),
    "/tmp/home/.config/microsoft-edge",
  );
  assert.equal(
    defaultUserDataDir({
      platform: "linux",
      kind: "chrome",
      env: { ...linuxEnv, XDG_CONFIG_HOME: "/tmp/xdg" },
    }),
    "/tmp/xdg/google-chrome",
  );
  assert.equal(
    defaultUserDataDir({
      platform: "linux",
      kind: "chrome",
      env: { ...linuxEnv, CHROME_CONFIG_HOME: "/tmp/chrome-home" },
    }),
    "/tmp/chrome-home/google-chrome",
  );
});

test("the default check ignores trailing separators and is Linux-only", () => {
  assert.equal(
    isDefaultProfile({
      platform: "linux",
      kind: "edge",
      profile: "/tmp/home/.config/microsoft-edge/",
      env: linuxEnv,
    }),
    true,
  );
  assert.equal(
    isDefaultProfile({
      platform: "linux",
      kind: "chrome",
      profile: "/tmp/smoke-work/profile",
      env: linuxEnv,
    }),
    false,
  );
  assert.equal(defaultUserDataDir({ platform: "darwin", kind: "chrome", env: linuxEnv }), null);
  assert.equal(
    isDefaultProfile({
      platform: "win32",
      kind: "chrome",
      profile: "C:\\profiles\\x",
      env: linuxEnv,
    }),
    false,
  );
});

test("Linux product registration isolates the config home only for a default profile", () => {
  const isolated = browserRuntimeEnv({
    platform: "linux",
    kind: "chrome",
    productRegistration: true,
    profile: "/tmp/home/.config/google-chrome",
    env: linuxEnv,
    work,
  });
  assert.equal(isolated.isolated, true);
  assert.equal(isolated.configHome, path.join(work, "browser-config-home"));
  assert.equal(isolated.env.XDG_CONFIG_HOME, isolated.configHome);
  assert.equal(isolated.env.CHROME_CONFIG_HOME, isolated.configHome);
  assert.equal(isolated.env.HOME, linuxEnv.HOME);

  const untouched = browserRuntimeEnv({
    platform: "linux",
    kind: "chrome",
    productRegistration: true,
    profile: "/tmp/smoke-work/profile",
    env: linuxEnv,
    work,
  });
  assert.equal(untouched.isolated, false);
  assert.equal(untouched.env, linuxEnv);

  for (const overrides of [
    { platform: "linux", productRegistration: false },
    { platform: "darwin", productRegistration: true },
    { platform: "win32", productRegistration: true },
  ]) {
    const r = browserRuntimeEnv({
      kind: "chrome",
      profile: "/tmp/home/.config/google-chrome",
      env: linuxEnv,
      work,
      ...overrides,
    });
    assert.equal(r.isolated, false);
    assert.equal(r.env, linuxEnv);
  }
});
