import { Box } from "@mantine/core";
import { useMediaQuery } from "@mantine/hooks";
import { useUnit } from "effector-react";
import type React from "react";
import { useState } from "react";
import {
  $resourceList,
  $selectedResource,
  $selectedResourceType,
} from "@/entities/fhir";
import { ResourceDetails } from "@/features/resource-details";
import {
  Pagination,
  ResourceCard,
  ResourceSearchForm,
  ResourceTypeList,
} from "@/features/resource-list";
import { Splitter } from "@/shared/ui/Splitter";
import styles from "./ResourceBrowserPage.module.css";

export const ResourceBrowserPage: React.FC = () => {
  // URL state synchronization - temporarily disabled to fix infinite loop
  // useResourceBrowserUrlState();

  // Effector state
  const selectedResourceType = useUnit($selectedResourceType);
  const resourceList = useUnit($resourceList);
  const selectedResource = useUnit($selectedResource);

  // Responsive layout
  const isMobile = useMediaQuery("(max-width: 768px)");
  const isTablet = useMediaQuery("(max-width: 1024px)");

  // Layout state for mobile/tablet
  const [mobileView, setMobileView] = useState<"types" | "list" | "details">("types");

  // Resource list is automatically updated via the resource browser store

  // Handle edit resource
  const handleEditResource = (resource: any) => {
    // TODO: Implement resource editing
    console.log("Edit resource:", resource);
  };

  // Mobile layout - single view at a time
  if (isMobile) {
    return (
      <Box className={styles.container}>
        <Box className={styles.mobileHeader}>
          <div className={styles.mobileNav}>
            <button
              type="button"
              className={`${styles.navButton} ${mobileView === "types" ? styles.active : ""}`}
              onClick={() => setMobileView("types")}
            >
              Types
            </button>
            <button
              type="button"
              className={`${styles.navButton} ${mobileView === "list" ? styles.active : ""}`}
              onClick={() => setMobileView("list")}
              disabled={!selectedResourceType}
            >
              List ({resourceList.total || 0})
            </button>
            <button
              type="button"
              className={`${styles.navButton} ${mobileView === "details" ? styles.active : ""}`}
              onClick={() => setMobileView("details")}
              disabled={!selectedResource}
            >
              Details
            </button>
          </div>
        </Box>

        <Box className={styles.mobileContent}>
          {mobileView === "types" && (
            <ResourceTypeList
              className={styles.resourceTypes}
              onResourceTypeSelect={() => setMobileView("list")}
            />
          )}

          {mobileView === "list" && selectedResourceType && (
            <Box className={styles.resourceList}>
              <ResourceSearchForm />
              <Box className={styles.listContent}>
                {resourceList.data.map((resource) => (
                  <ResourceCard
                    key={resource.id}
                    resource={resource}
                    isSelected={selectedResource?.id === resource.id}
                  />
                ))}
              </Box>
              <Pagination />
            </Box>
          )}

          {mobileView === "details" && selectedResource && (
            <ResourceDetails onEdit={handleEditResource} />
          )}
        </Box>
      </Box>
    );
  }

  // Tablet layout - two panels with collapsible sidebar
  if (isTablet) {
    return (
      <Box className={styles.container}>
        <Splitter
          direction="horizontal"
          defaultSize={30}
          minSize={20}
          maxSize={80}
          className={styles.tabletSplitter}
        >
          <Box className={styles.leftPanel}>
            <ResourceTypeList className={styles.resourceTypes} />
            {selectedResourceType && (
              <Box className={styles.resourceList}>
                <ResourceSearchForm />
                <Box className={styles.listContent}>
                  {resourceList.data.map((resource) => (
                    <ResourceCard
                      key={resource.id}
                      resource={resource}
                      isSelected={selectedResource?.id === resource.id}
                    />
                  ))}
                </Box>
                <Pagination />
              </Box>
            )}
          </Box>
          <ResourceDetails onEdit={handleEditResource} />
        </Splitter>
      </Box>
    );
  }

  // Desktop layout - three panels
  return (
    <Box className={styles.container}>
      <Splitter
        direction="horizontal"
        defaultSize={25}
        minSize={15}
        maxSize={40}
        className={styles.desktopSplitter}
      >
        {/* Resource Types Sidebar */}
        <ResourceTypeList className={styles.resourceTypes} />

        {/* Right Panel containing List and Details */}
        <Splitter
          direction="horizontal"
          defaultSize={50}
          minSize={30}
          maxSize={70}
          className={styles.rightPanel}
        >
          {/* Resource List Panel */}
          <Box className={styles.middlePanel}>
            <ResourceSearchForm />
            <Box className={styles.listContent}>
              {selectedResourceType ? (
                resourceList.data.map((resource) => (
                  <ResourceCard
                    key={resource.id}
                    resource={resource}
                    isSelected={selectedResource?.id === resource.id}
                  />
                ))
              ) : (
                <Box className={styles.emptyState}>Select a resource type to browse resources</Box>
              )}
            </Box>
            <Pagination />
          </Box>

          {/* Resource Details Panel */}
          <ResourceDetails onEdit={handleEditResource} />
        </Splitter>
      </Splitter>
    </Box>
  );
};
