import path from "node:path";

// Chrome 136+ refuses --remote-debugging-pipe when the user-data directory is the browser's
// default. The packaged smoke registers the host in the isolated per-user location, so on Linux
// that profile *is* the default and the browser would never answer DevTools. The product has no
// such limitation (native messaging is not DevTools); only this test needs a non-default
// comparison, which an isolated config home provides while --user-data-dir stays the product
// directory the manifest lives in.
const BROWSER_DIR = { chrome: "google-chrome", edge: "microsoft-edge", chromium: "chromium" };

const linuxPath = (p) => path.posix.resolve(path.posix.normalize(p));

export function defaultUserDataDir({ platform, kind, env }) {
  if (platform !== "linux") return null;
  const configHome =
    env.CHROME_CONFIG_HOME || env.XDG_CONFIG_HOME || path.posix.join(env.HOME ?? "", ".config");
  return path.posix.join(configHome, BROWSER_DIR[kind] ?? BROWSER_DIR.chrome);
}

export function isDefaultProfile({ platform, kind, profile, env }) {
  const dir = defaultUserDataDir({ platform, kind, env });
  return dir !== null && linuxPath(dir) === linuxPath(profile);
}

export function browserRuntimeEnv({ platform, kind, productRegistration, profile, env, work }) {
  if (
    platform !== "linux" ||
    !productRegistration ||
    !isDefaultProfile({ platform, kind, profile, env })
  ) {
    return { env, isolated: false, configHome: null };
  }
  const configHome = path.join(work, "browser-config-home");
  return {
    env: { ...env, XDG_CONFIG_HOME: configHome, CHROME_CONFIG_HOME: configHome },
    isolated: true,
    configHome,
  };
}
