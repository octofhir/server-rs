// System queries (health, build info, resource types, SQL, GraphQL, operations)

// Auth queries (login, logout, user info)
export {
  authKeys,
  useAuth,
  useCurrentUser,
  useLogin,
  useLogout,
} from "./useAuth";
// Auth interceptor (global error handling)
export { useAuthInterceptor } from "./useAuthInterceptor";
// DB Console queries (history, tables, active queries, index management)
export {
  dbConsoleKeys,
  useActiveQueries,
  useClearHistory,
  useDbTables,
  useDropIndex,
  useQueryHistory,
  useRunMaintenance,
  useSaveHistory,
  useTableDetail,
  useTerminateQuery,
} from "./useDbConsoleQueries";
// FHIR queries (CRUD operations)
export {
  fhirKeys,
  useCapabilities,
  useCreateResource,
  useDeleteResource,
  useFollowBundleLink,
  useResource,
  useResourceSearch,
  useUpdateResource,
} from "./useFhirQueries";
// Formatter configuration (LSP SQL formatting settings)
export {
  formatterKeys,
  useFormatterConfig,
  useFormatterSettings,
  useSaveFormatterConfig,
} from "./useFormatterConfig";

// Package queries (FHIR package management)
export {
  packageKeys,
  useInstallPackage,
  useInstallPackageWithProgress,
  usePackageDetails,
  usePackageFhirSchema,
  usePackageLookup,
  usePackageResourceContent,
  usePackageResources,
  usePackageSearch,
  usePackages,
} from "./usePackageQueries";
// Server-backed preference/history user scoping
export { usePreferenceUserSync } from "./usePreferenceUserSync";
export {
  systemKeys,
  useBuildInfo,
  useGraphQLMutation,
  useGraphQLSchema,
  useHealth,
  useJsonSchema,
  useOperation,
  useOperations,
  useResourceTypes,
  useResourceTypesCategorized,
  useSettings,
  useSqlMutation,
  useUpdateOperation,
} from "./useSystemQueries";
