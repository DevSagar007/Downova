// The few extension APIs this extension uses (Chrome 116+), declared locally instead of pulling
// in the full @types/chrome surface. `downloads` and `webRequest` are optional permissions
// (B2.13): their namespaces are undefined until granted.
declare namespace chrome {
  interface Event<F> {
    addListener(cb: F, ...extra: unknown[]): void;
    removeListener(cb: F): void;
    hasListener(cb: F): boolean;
  }
  interface DownloadItem {
    id: number;
    url: string;
    finalUrl: string;
    mime: string;
    totalBytes: number;
    filename: string;
    danger: string;
    incognito: boolean;
    byExtensionId?: string;
    state: "in_progress" | "interrupted" | "complete";
    startTime: string;
    error?: string;
  }
  interface DownloadDelta {
    id: number;
    state?: { current?: string };
    error?: { current?: string };
    filename?: { current?: string };
    danger?: { current?: string };
  }
  const downloads:
    | {
        onCreated: Event<(item: DownloadItem) => void>;
        onChanged: Event<(delta: DownloadDelta) => void>;
        search(query: { id: number }): Promise<DownloadItem[]>;
        cancel(id: number): Promise<void>;
      }
    | undefined;
  interface WebRequestDetails {
    requestId: string;
    url: string;
    method: string;
    type: string;
    tabId: number;
    statusCode?: number;
    redirectUrl?: string;
    requestHeaders?: { name: string; value?: string }[];
  }
  const webRequest:
    | {
        onSendHeaders: Event<(d: WebRequestDetails) => void>;
        onBeforeRedirect: Event<(d: WebRequestDetails) => void>;
        onHeadersReceived: Event<(d: WebRequestDetails) => void>;
      }
    | undefined;
  namespace permissions {
    interface Permissions {
      permissions?: string[];
      origins?: string[];
    }
    function contains(p: Permissions): Promise<boolean>;
    function request(p: Permissions): Promise<boolean>;
    function remove(p: Permissions): Promise<boolean>;
    function getAll(): Promise<Permissions>;
    const onAdded: Event<(p: Permissions) => void>;
    const onRemoved: Event<(p: Permissions) => void>;
  }
  namespace i18n {
    function getUILanguage(): string;
  }
  namespace runtime {
    const lastError: { message?: string } | undefined;
    function sendNativeMessage(
      application: string,
      message: object,
      callback: (response: unknown) => void,
    ): void;
    function sendMessage(message: object): Promise<unknown>;
    const onInstalled: { addListener(cb: () => void): void };
    const onStartup: { addListener(cb: () => void): void };
    const onMessage: {
      addListener(
        cb: (
          message: unknown,
          sender: { id?: string },
          respond: (r: unknown) => void,
        ) => boolean | undefined,
      ): void;
    };
    const id: string;
    function openOptionsPage(): Promise<void>;
  }
  namespace contextMenus {
    function create(props: {
      id: string;
      title: string;
      contexts: string[];
      targetUrlPatterns?: string[];
    }): void;
    function removeAll(): Promise<void>;
    const onClicked: {
      addListener(cb: (info: { menuItemId: string | number; linkUrl?: string }) => void): void;
    };
  }
  namespace storage {
    const local: {
      get(key: string): Promise<Record<string, unknown>>;
      set(items: Record<string, unknown>): Promise<void>;
    };
    /** Memory-only, survives worker restarts, cleared when the browser closes. */
    const session: {
      get(key: string): Promise<Record<string, unknown>>;
      set(items: Record<string, unknown>): Promise<void>;
    };
    const onChanged: {
      addListener(cb: (changes: Record<string, unknown>, area: string) => void): void;
    };
  }
  namespace action {
    function setBadgeText(details: { text: string }): Promise<void>;
    function setBadgeBackgroundColor(details: { color: string }): Promise<void>;
  }
}
