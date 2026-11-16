# Downova privacy policy

- **Applies to:** the Downova desktop app and the Downova browser extension (Chrome, Edge, Firefox).
- **Last updated:** 2026-11-16.

Downova is a free download manager that runs on your own computer. It has **no accounts, no ads, no analytics and no
telemetry**. Downova's developers run **no server** that receives your data.

## What leaves your computer

Only the requests a download needs, sent **directly to the servers you download from**:
- web servers (HTTP/HTTPS);
- FTP and SFTP servers;
- a proxy, if you configure one.

Nothing is sent to Downova's developers or to anyone else.

**If you use Help → Check for updates:** in a build that has an update server configured, Downova asks that server
whether a newer version exists, and downloads it only when you agree. Builds without one say "Updates are not
configured" and contact nothing. Automatic checking is off.

## The browser extension

The extension passes the links you choose to the Downova app **on the same computer**, through the browser's
native messaging. It never sends them anywhere else.

- **The right-click menu ("Download with Downova"):** sends that link to the app.
- **Automatic capture** (Chrome and Edge, off by default):
  - it is turned on in the extension's options, only for the sites you add;
  - it hands plain public downloads from those sites to the app;
  - it uses the optional `downloads` and `webRequest` permissions and a site permission for each site you add, and
    the browser asks you for each.
- **Signed-in downloads** (Chrome and Edge, one explicit menu action):
  - with the optional `cookies` permission, the cookies of **that one site** are passed to the app for that
    download only;
  - they are kept in memory, or in your operating system's credential store if you choose "remember";
  - they are never written to logs, history or exports.
- **Private (incognito/InPrivate) windows are never captured.**
- **Stored by the extension** (`storage.local`, on your computer only):
  - the last 20 hand-offs: the link shown with its query removed, a request ID and the outcome;
  - capture records;
  - finished entries are deleted after 7 days.

Firefox lists this as "browsing activity" because the link you choose leaves the extension for the local app.

## The desktop app

The app keeps its data in your user profile (`%LOCALAPPDATA%\Downova`, `~/Library/Application Support/Downova` or
`~/.local/share/downova`). Only you can read it:
- the download list: each link, file name, folder and state;
- download history, which you can limit in Options;
- settings.

**Secrets** go only to your operating system's credential store (Windows Credential Manager, macOS Keychain, Secret
Service): site logins, a proxy password, remembered sign-ins. They never appear in logs, history or exports.

**Diagnostics** (Help → Diagnostics…) builds a redacted report that you preview and save yourself. Nothing is sent.

## Your choices

- **Remove downloads from the list:** downloaded files stay.
- **Clear history** in Options.
- **Forget a remembered sign-in** in the download's details.
- **Uninstall:**
  - it removes the program;
  - your data folder and your downloaded files stay until you delete them;
  - removing the extension deletes its stored records.

## Contact

Questions about this policy: open an issue in the project's repository.
