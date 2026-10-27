import { invoke } from "@tauri-apps/api/core";
import LicensePanel from "../../license/LicensePanel";
import { activationOutcome, type LicenseStatus } from "../../license/status";
import { useAppCtx } from "../../appShell";

export default function License() {
  const {
    buyYap, license, setBuyPrompt, setLicense, toast, toastError,
  } = useAppCtx();
  async function activateLicense(key: string) {
    const status = await invoke<LicenseStatus>("activate_license", { key });
    setLicense(status);
    // A grace key restores the trial; it does not license anything, so the
    // message comes from the returned status, not from "the call succeeded".
    const outcome = activationOutcome(status);
    if (outcome.closeBuyPrompt) setBuyPrompt(false);
    toast(outcome.toast);
  }
  async function deactivateLicense() {
    try {
      setLicense(await invoke<LicenseStatus>("deactivate_license"));
      toast("License removed from this Mac");
    } catch (e) {
      toastError(e);
    }
  }
  return (
                <LicensePanel
                  status={license}
                  onBuy={buyYap}
                  onActivate={activateLicense}
                  onDeactivate={deactivateLicense}
                />
  );
}
