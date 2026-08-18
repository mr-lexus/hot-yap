import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import BrandLogo from "./BrandLogo";
import Icon from "./Icons";
import type { Theme } from "./appearance";

interface TitleBarProps {
  theme: Theme;
}

export default function TitleBar({ theme }: TitleBarProps) {
  const { t } = useTranslation();
  const [maximized, setMaximized] = useState(false);
  const [focused, setFocused] = useState(true);

  useEffect(() => {
    let unlisten: Array<() => void> = [];
    let disposed = false;

    (async () => {
      try {
        const window = getCurrentWindow();
        if (disposed) return;
        setMaximized(await window.isMaximized());
        setFocused(await window.isFocused());

        const un1 = await window.onResized(() => {
          void window.isMaximized().then(setMaximized).catch(() => {});
        });
        const un2 = await window.onFocusChanged(({ payload }) => setFocused(payload));
        unlisten = [un1, un2];
      } catch (e) {
        console.error("titlebar window state failed:", e);
      }
    })();

    return () => {
      disposed = true;
      unlisten.forEach((un) => un());
    };
  }, []);

  const minimize = () => void getCurrentWindow().minimize();
  const toggleMaximize = () => {
    const window = getCurrentWindow();
    void (maximized ? window.unmaximize() : window.maximize());
  };
  const close = () => void getCurrentWindow().close();

  return (
    <header className={`titlebar ${focused ? "" : "unfocused"}`} data-tauri-drag-region>
      <div className="titlebar-brand" data-tauri-drag-region>
        <span className="titlebar-logo" data-tauri-drag-region>
          <BrandLogo size={20} variant={theme === "dark" ? "light" : "dark"} />
        </span>
        <span className="titlebar-name" data-tauri-drag-region>{t("brand.name")}</span>
        <span className="titlebar-tagline" data-tauri-drag-region>{t("brand.kicker")}</span>
      </div>
      <div className="titlebar-controls">
        <button
          className="titlebar-control"
          onClick={minimize}
          title={t("window.minimize")}
          aria-label={t("window.minimize")}
        >
          <Icon name="minimize" size={15} />
        </button>
        <button
          className="titlebar-control"
          onClick={toggleMaximize}
          title={t(maximized ? "window.restore" : "window.maximize")}
          aria-label={t(maximized ? "window.restore" : "window.maximize")}
        >
          <Icon name={maximized ? "restore" : "maximize"} size={13} />
        </button>
        <button
          className="titlebar-control titlebar-control-close"
          onClick={close}
          title={t("window.close")}
          aria-label={t("window.close")}
        >
          <Icon name="close" size={15} />
        </button>
      </div>
    </header>
  );
}