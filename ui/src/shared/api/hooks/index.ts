// System queries (health, build info, resource types, SQL, GraphQL, operations)
export {
	systemKeys,
	useHealth,
	useBuildInfo,
	useResourceTypes,
	useResourceTypesCategorized,
	useJsonSchema,
	useSqlMutation,
	useGraphQLMutation,
	useGraphQLSchema,
	useOperations,
	useOperation,
	useUpdateOperation,
} from "./useSystemQueries";

// FHIR queries (CRUD operations)
export {
	fhirKeys,
	useCapabilities,
	useResource,
	useResourceSearch,
	useCreateResource,
	useUpdateResource,
	useDeleteResource,
	useFollowBundleLink,
} from "./useFhirQueries";

// Auth queries (login, logout, user info)
export {
	authKeys,
	useCurrentUser,
	useLogin,
	useLogout,
	useAuth,
} from "./useAuth";

// Auth interceptor (global error handling)
export { useAuthInterceptor } from "./useAuthInterceptor";

// Package queries (FHIR package management)
export {
	packageKeys,
	usePackages,
	usePackageDetails,
	usePackageResources,
	usePackageResourceContent,
	usePackageFhirSchema,
	usePackageLookup,
	usePackageSearch,
	useInstallPackage,
	useInstallPackageWithProgress,
} from "./usePackageQueries";

// Formatter configuration (LSP SQL formatting settings)
export {
	formatterKeys,
	useFormatterConfig,
	useSaveFormatterConfig,
	useFormatterSettings,
} from "./useFormatterConfig";
