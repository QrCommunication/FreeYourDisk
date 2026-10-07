const target = document.getElementById("app");

const STARTUP_MESSAGE = "Démarrage de FreeYourDisk…";
const STARTUP_FAILURE_MESSAGE =
  "FreeYourDisk n’a pas pu démarrer. Relancez l’application et consultez ses journaux si le problème persiste.";
const STARTUP_TIMEOUT_MS = 8_000;

function createFallback(message: string, isFailure = false): HTMLElement {
  const fallback = document.createElement("section");
  fallback.dataset.appFallback = isFailure ? "error" : "loading";
  fallback.setAttribute("role", isFailure ? "alert" : "status");
  fallback.setAttribute("aria-live", "polite");
  fallback.style.cssText = [
    "box-sizing:border-box",
    "display:grid",
    "min-height:100vh",
    "place-items:center",
    "margin:0",
    "padding:2rem",
    "background:#0b1018",
    "color:#f8fafc",
    "font-family:system-ui,sans-serif",
    "text-align:center",
  ].join(";");

  const messageElement = document.createElement("p");
  messageElement.textContent = message;
  messageElement.style.cssText =
    "max-width:32rem;margin:0;font-size:1rem;line-height:1.5";
  fallback.append(messageElement);

  return fallback;
}

if (!target) {
  console.error("FreeYourDisk frontend could not find its mount target.");
} else {
  let fallback = createFallback(STARTUP_MESSAGE);
  let bootstrapped = false;
  let startupFailed = false;
  let startupTimeout: ReturnType<typeof window.setTimeout> | undefined;

  target.replaceChildren(fallback);

  function clearStartupTimeout(): void {
    if (startupTimeout === undefined) {
      return;
    }

    window.clearTimeout(startupTimeout);
    startupTimeout = undefined;
  }

  function showStartupFailure(error: unknown): void {
    clearStartupTimeout();
    console.error("FreeYourDisk frontend failed to start.", error);

    if (startupFailed) {
      return;
    }

    startupFailed = true;
    fallback = createFallback(STARTUP_FAILURE_MESSAGE, true);
    target.replaceChildren(fallback);
  }

  window.addEventListener("error", (event) => {
    console.error(
      "FreeYourDisk frontend runtime error.",
      event.error ?? event.message,
    );

    if (!bootstrapped) {
      showStartupFailure(event.error ?? event.message);
    }
  });

  window.addEventListener("unhandledrejection", (event) => {
    console.error("FreeYourDisk frontend unhandled rejection.", event.reason);

    if (!bootstrapped) {
      showStartupFailure(event.reason);
    }
  });

  async function bootstrap(): Promise<void> {
    try {
      const [{ mount }, { default: App }] = await Promise.all([
        import("svelte"),
        import("./App.svelte"),
        import("./app.css"),
        import("./lib/i18n"),
      ]);

      mount(App, { target });
      bootstrapped = true;
      clearStartupTimeout();
      fallback.remove();
    } catch (error) {
      showStartupFailure(error);
    }
  }

  startupTimeout = window.setTimeout(() => {
    if (!bootstrapped) {
      showStartupFailure(new Error("FreeYourDisk frontend startup timed out."));
    }
  }, STARTUP_TIMEOUT_MS);

  void bootstrap();
}
