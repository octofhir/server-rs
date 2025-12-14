// System queries (health, build info, resource types, SQL, GraphQL, operations)
export {
	systemKeys,
	useHealth,
	useBuildInfo,
	useResourceTypes,
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
