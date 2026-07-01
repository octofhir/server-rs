import { Badge, Collapse, Text, useDisclosure } from "@octofhir/ui-kit";
import { useUnit } from "effector-react";
import { ChevronRight } from "lucide-react";
import { $body, $customHeaders } from "../state/consoleStore";
import { RequestOptionTabs } from "./RequestBuilderAccordion";
import styles from "./RequestOptionsStrip.module.css";

interface RequestOptionsStripProps {
  resourceType?: string;
}

/**
 * Compact, collapsed-by-default disclosure for request Headers/Body.
 * Replaces the old fixed 32% split pane — reclaims vertical space for the
 * response, which is the primary surface in the console.
 */
export function RequestOptionsStrip({ resourceType }: RequestOptionsStripProps) {
  const [opened, { toggle }] = useDisclosure(false);
  const { customHeaders, body } = useUnit({
    customHeaders: $customHeaders,
    body: $body,
  });

  const headerCount = Object.keys(customHeaders).length;
  const hasBody = body.trim().length > 0;

  return (
    <div className={styles.root} data-open={opened}>
      <button type="button" className={styles.header} onClick={toggle}>
        <ChevronRight size={14} className={styles.chevron} data-open={opened} />
        <Text variant="body-2" className={styles.label}>
          Request options
        </Text>
        <span className={styles.summary}>
          <span className={styles.summaryItem}>
            Headers
            {headerCount > 0 && (
              <Badge size="sm" theme="info" className={styles.badge}>
                {headerCount}
              </Badge>
            )}
          </span>
          <span className={styles.summaryItem}>
            Body
            {hasBody && (
              <Badge size="sm" theme="warning" className={styles.badge}>
                1
              </Badge>
            )}
          </span>
        </span>
      </button>

      <Collapse in={opened}>
        <div className={styles.body}>
          <RequestOptionTabs resourceType={resourceType} />
        </div>
      </Collapse>
    </div>
  );
}
