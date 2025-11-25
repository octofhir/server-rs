import { type Component, Show } from "solid-js";
import { useUnit } from "effector-solid";
import { $pagination, navigateToPageFx, setPageCount } from "@/entities/fhir";
import { Select } from "@/shared/ui";
import { IconChevronLeft, IconChevronRight } from "@/shared/ui/Icon";
import styles from "./Pagination.module.css";

interface PaginationProps {
  class?: string;
}

const PAGE_SIZE_OPTIONS = [
  { value: "10", label: "10 per page" },
  { value: "20", label: "20 per page" },
  { value: "50", label: "50 per page" },
  { value: "100", label: "100 per page" },
];

// Double chevron icons
const IconChevronsLeft: Component<{ size?: number }> = (props) => (
  <svg
    width={props.size || 16}
    height={props.size || 16}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
  >
    <path d="m11 17-5-5 5-5" />
    <path d="m18 17-5-5 5-5" />
  </svg>
);

const IconChevronsRight: Component<{ size?: number }> = (props) => (
  <svg
    width={props.size || 16}
    height={props.size || 16}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
  >
    <path d="m6 17 5-5-5-5" />
    <path d="m13 17 5-5-5-5" />
  </svg>
);

export const Pagination: Component<PaginationProps> = (props) => {
  const pagination = useUnit($pagination);

  const handlePageSizeChange = (e: Event) => {
    const value = (e.target as HTMLSelectElement).value;
    if (value) {
      setPageCount(Number(value));
    }
  };

  const handleNavigation = (url: string) => {
    navigateToPageFx(url);
  };

  const links = () => pagination().links;
  const count = () => pagination().count;
  const hasAnyNavigation = () =>
    links().first || links().prev || links().next || links().last;

  return (
    <Show when={hasAnyNavigation() || count()}>
      <div class={`${styles.container} ${props.class || ""}`}>
        <div class={styles.pageSize}>
          <Select
            value={String(count())}
            onChange={handlePageSizeChange}
          >
            {PAGE_SIZE_OPTIONS.map((opt) => (
              <option value={opt.value}>{opt.label}</option>
            ))}
          </Select>
        </div>

        <Show when={hasAnyNavigation()}>
          <div class={styles.navigation}>
            <button
              class={styles.navButton}
              disabled={!links().first}
              onClick={() => links().first && handleNavigation(links().first!)}
              title="First page"
            >
              <IconChevronsLeft size={16} />
            </button>

            <button
              class={styles.navButton}
              disabled={!links().prev}
              onClick={() => links().prev && handleNavigation(links().prev!)}
              title="Previous page"
            >
              <IconChevronLeft size={16} />
            </button>

            <span class={styles.pageInfo}>Page navigation</span>

            <button
              class={styles.navButton}
              disabled={!links().next}
              onClick={() => links().next && handleNavigation(links().next!)}
              title="Next page"
            >
              <IconChevronRight size={16} />
            </button>

            <button
              class={styles.navButton}
              disabled={!links().last}
              onClick={() => links().last && handleNavigation(links().last!)}
              title="Last page"
            >
              <IconChevronsRight size={16} />
            </button>
          </div>
        </Show>
      </div>
    </Show>
  );
};
