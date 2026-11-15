import { NumberField, Row, Section, Toggle } from "../components";
import type { Config } from "../../shared/api";
import { useT } from "../../shared/i18n";

interface Props {
  config: Config;
  update: (mutate: (draft: Config) => void) => void;
}

/** 「感应锁屏」页：手机离开身边后自动锁定电脑。 */
export function Proximity({ config, update }: Props) {
  const t = useT();
  const { enabled, lock_delay_secs } = config.proximity;

  return (
    <>
      <Section title={t.proximity.title}>
        <Row label={t.proximity.enable} hint={t.proximity.enableHint}>
          <Toggle
            checked={enabled}
            onChange={(value) =>
              update((draft) => {
                draft.proximity.enabled = value;
              })
            }
          />
        </Row>
        <Row label={t.proximity.delay} hint={t.proximity.delayHint}>
          <NumberField
            value={lock_delay_secs}
            min={5}
            max={600}
            step={5}
            suffix={t.proximity.seconds}
            disabled={!enabled}
            onChange={(value) =>
              update((draft) => {
                draft.proximity.lock_delay_secs = value;
              })
            }
          />
        </Row>
      </Section>
    </>
  );
}
