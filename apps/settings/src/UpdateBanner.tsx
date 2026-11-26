import type { UpdateInfo } from "./config";
import type es from "./i18n/es.json";

type Texts = typeof es;

interface UpdateBannerProps {
  t: Texts;
  info: UpdateInfo;
  /** Download share from 0 to 1 while installing, `null` if the size is unknown. */
  progress: number | null;
  installing: boolean;
  failed: boolean;
  onInstall: () => void;
  onLater: () => void;
  onNotes: () => void;
}

/**
 * The software-update card that sits above every section while an update is
 * waiting, as in macOS System Settings: it never blocks the window, and
 * "Later" only hides it until Settings is opened again.
 */
export function UpdateBanner({
  t,
  info,
  progress,
  installing,
  failed,
  onInstall,
  onLater,
  onNotes,
}: UpdateBannerProps) {
  const percent = progress === null ? null : Math.round(progress * 100);
  const detail = installing
    ? percent === null
      ? t.updateDownloadingUnknown
      : t.updateDownloading.replace("{percent}", String(percent))
    : failed
      ? t.updateInstallError
      : t.updateBannerBody;

  return (
    <section className="update" role="status" aria-live="polite">
      <span className="update-icon" aria-hidden="true">
        ↓
      </span>
      <div className="update-text">
        <p className="update-title">{t.updateBannerTitle.replace("{version}", info.version)}</p>
        <p className={failed ? "update-detail update-detail-error" : "update-detail"}>{detail}</p>
        {installing ? (
          <div
            className={percent === null ? "update-progress is-unknown" : "update-progress"}
            role="progressbar"
            aria-label={t.updateNow}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent ?? undefined}
          >
            <span style={{ width: `${percent ?? 35}%` }} />
          </div>
        ) : null}
      </div>
      {installing ? null : (
        <div className="update-actions">
          {info.notes ? (
            <button type="button" className="update-link" onClick={onNotes}>
              {t.updateWhatsNew}
            </button>
          ) : null}
          {failed ? null : (
            <button type="button" className="push" onClick={onLater}>
              {t.updateLater}
            </button>
          )}
          <button type="button" className="push push-primary" onClick={onInstall}>
            {failed ? t.updateRetry : t.updateNow}
          </button>
        </div>
      )}
    </section>
  );
}
