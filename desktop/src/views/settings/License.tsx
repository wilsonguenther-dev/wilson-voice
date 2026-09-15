import { invoke } from "@tauri-apps/api/core";
import { errorText } from "../../errors";
import LicensePanel from "../../license/LicensePanel";
import { type LicenseStatus } from "../../license/status";
import { useAppCtx } from "../../appShell";

export default function License() {
  const {
    buyYap, license, setBuyPrompt, setLicense, toast,
  } = useAppCtx();
  async function activateLicense(key: string) {
    const status = await invoke<LicenseStatus>("activate_license", { key });
    setLicense(status);
    setBuyPrompt(false);
    toast("Yap is licensed on this Mac");
  }
  async function deactivateLicense() {
    try {
      setLicense(await invoke<LicenseStatus>("deactivate_license"));
      toast("License removed from this Mac");
    } catch (e) {
      toast(errorText(e));
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
