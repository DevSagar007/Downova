// The few extension APIs this extension uses (Chrome 116+), declared locally instead of pulling
// in the full @types/chrome surface.
declare namespace chrome {
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
    const onChanged: {
      addListener(cb: (changes: Record<string, unknown>, area: string) => void): void;
    };
  }
  namespace action {
    function setBadgeText(details: { text: string }): Promise<void>;
    function setBadgeBackgroundColor(details: { color: string }): Promise<void>;
  }
}
