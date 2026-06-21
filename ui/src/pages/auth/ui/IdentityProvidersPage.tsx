import { Alert, Card, Field, Form, useDebouncedValue, useDisclosure } from "@octofhir/ui-kit";
import { useState } from "react";
import {
	Text,
	Button,
	TextInput,
	DataPreview,
	Badge,
	EmptyState,
	Modal,
	Skeleton,
	Switch,
	Select,
	PasswordInput,
	MultiSelect,
} from "@octofhir/ui-kit";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import { DropdownMenu } from "@octofhir/ui-kit";
import { Plus, Search as Magnifier, EllipsisVertical, Pencil, Trash2 as TrashBin, Globe } from "lucide-react";
import {
	getIdentityProviderStatusView,
	getIdentityProviderTypeView,
	identityProviderTypeOptions,
	type IdentityProviderResource,
	type IdentityProviderType,
} from "@/entities/identity-provider";
import { useIdentityProviders, useCreateIdentityProvider, useUpdateIdentityProvider, useDeleteIdentityProvider } from "../lib/useIdentityProviders";
import { getBundleResources } from "@/shared/api/guards";
import classes from "./IdentityProvidersPage.module.css";

export function IdentityProvidersPage() {
	const [search, setSearch] = useState("");
	const [debouncedSearch] = useDebouncedValue(search, 500);
	const [opened, { open, close }] = useDisclosure(false);
	const [editingIdp, setEditingIdp] = useState<IdentityProviderResource | null>(null);
	const [deleteTarget, setDeleteTarget] = useState<IdentityProviderResource | null>(null);

	const { data, isLoading, isError, error, refetch } = useIdentityProviders({ search: debouncedSearch });
	const deleteIdp = useDeleteIdentityProvider();

	const handleEdit = (idp: IdentityProviderResource) => {
		setEditingIdp(idp);
		open();
	};

	const handleDeleteConfirm = () => {
		if (deleteTarget?.id) {
			deleteIdp.mutate(deleteTarget.id, {
				onSuccess: () => setDeleteTarget(null),
			});
		}
	};

	const handleClose = () => {
		setEditingIdp(null);
		close();
	};

	const providers = getBundleResources<IdentityProviderResource>(data);
	const isFiltered = debouncedSearch.length > 0;

	return (
		<WorkspacePageLayout
			title="Identity Providers"
			description="Manage external OIDC/OAuth2 authentication providers"
			actions={
				<Button variant="filled" onClick={open}>
					<Button.Icon>
						<Plus width={16} />
					</Button.Icon>
					Add Provider
				</Button>
			}
			toolbar={
				<div className={classes.toolbar}>
					<TextInput
						aria-label="Search identity providers by name"
						placeholder="Search by name..."
						leftSection={<Magnifier width={16} />}
						value={search}
						onChange={(value) => setSearch(value)}
						className={classes.search}
					/>
				</div>
			}
		>

			<Card className={classes.tableContainer}>
				{isLoading ? (
					<div className={classes.skeletonList}>
						{["a", "b", "c", "d", "e"].map((k) => (
							<Skeleton key={k} className={classes.skeletonRow} />
						))}
					</div>
				) : isError ? (
					<EmptyState
						title="Failed to load providers"
						description={error instanceof Error ? error.message : "Something went wrong while loading identity providers."}
						actions={[
							<Button key="retry" variant="filled" onClick={() => refetch()}>
								Retry
							</Button>,
						]}
					/>
				) : providers.length === 0 ? (
					<EmptyState
						title={isFiltered ? "No matching providers" : "No identity providers yet"}
						description={
							isFiltered
								? "No providers match your search. Try a different term."
								: "Connect an external OIDC or OAuth2 provider to enable federated sign-in."
						}
						actions={
							isFiltered
								? [
										<Button key="clear" variant="outline" onClick={() => setSearch("")}>
											Clear filters
										</Button>,
									]
								: [
										<Button key="create" variant="filled" onClick={open}>
											Add Provider
										</Button>,
									]
						}
					/>
				) : (
					<DataPreview
						columns={[
							{ id: "provider", label: "Name / Issuer" },
							{ id: "type", label: "Type", width: 130 },
							{ id: "status", label: "Status", width: 110 },
							{ id: "actions", label: "", width: 48 },
						]}
						rows={providers.map((provider) => {
							const typeView = getIdentityProviderTypeView(provider.type);
							const statusView = getIdentityProviderStatusView(provider);

							return {
								provider: (
									<div className={classes.providerCell}>
										<Globe width={16} height={16} className={classes.providerIcon} aria-hidden="true" />
										<div className={classes.providerText}>
											<Text variant="body-2" className={classes.providerName}>
												<strong>{provider.name}</strong>
											</Text>
											<Text variant="caption-2" color="secondary" className={classes.providerIssuer}>
												{provider.issuer}
											</Text>
										</div>
									</div>
								),
								type: <Badge color={typeView.color}>{typeView.label}</Badge>,
								status: <Badge color={statusView.color}>{statusView.label}</Badge>,
								actions: (
									<DropdownMenu
										size="sm"
										icon={<EllipsisVertical width={16} />}
										defaultSwitcherProps={{
											variant: "subtle",
											size: "sm",
											"aria-label": "Provider actions",
										}}
										popupProps={{ placement: "bottom-end" }}
										items={[
											{
												text: "Edit",
												iconStart: <Pencil width={14} />,
												action: () => handleEdit(provider),
											},
											[
												{
													text: "Delete",
													iconStart: <TrashBin width={14} />,
													theme: "danger",
													action: () => setDeleteTarget(provider),
												},
											],
										]}
									/>
								),
							};
						})}
						getRowKey={(_row, index) => providers[index]?.id ?? providers[index]?.name ?? `${index}`}
					/>
				)}
			</Card>

			<IdpModal
				opened={opened}
				onClose={handleClose}
				idp={editingIdp}
			/>

			<DeleteIdpModal
				opened={!!deleteTarget}
				onClose={() => setDeleteTarget(null)}
				onConfirm={handleDeleteConfirm}
				providerName={deleteTarget?.name ?? ""}
				isDeleting={deleteIdp.isPending}
			/>
		</WorkspacePageLayout>
	);
}

function DeleteIdpModal({
	opened,
	onClose,
	onConfirm,
	providerName,
	isDeleting,
}: {
	opened: boolean;
	onClose: () => void;
	onConfirm: () => void;
	providerName: string;
	isDeleting: boolean;
}) {
	return (
		<Modal opened={opened} onClose={onClose} title="Delete Identity Provider" size="md">
			<div className={classes.deleteModalContent}>
				<Text variant="body-2">
					You are about to delete the identity provider: <strong>{providerName}</strong>
				</Text>

				<Alert
					theme="danger"
					title="This action cannot be undone."
					message="Users who sign in through this provider will no longer be able to authenticate."
				/>

				<div className={classes.formActions}>
					<Button variant="subtle" onClick={onClose} disabled={isDeleting}>
						Cancel
					</Button>
					<Button variant="subtle" color="red" onClick={onConfirm} loading={isDeleting}>
						Delete Provider
					</Button>
				</div>
			</div>
		</Modal>
	);
}

interface IdpFormValues {
	name: string;
	title: string;
	description: string;
	type: IdentityProviderType;
	issuer: string;
	clientId: string;
	clientSecret: string;
	authorizeUrl: string;
	tokenUrl: string;
	jwksUrl: string;
	userInfoUrl: string;
	scopes: string[];
	active: boolean;
}

const IDP_DEFAULTS: IdpFormValues = {
	name: "",
	title: "",
	description: "",
	type: "oidc",
	issuer: "",
	clientId: "",
	clientSecret: "",
	authorizeUrl: "",
	tokenUrl: "",
	jwksUrl: "",
	userInfoUrl: "",
	scopes: ["openid", "profile", "email"],
	active: true,
};

function validateIdp(values: IdpFormValues) {
	const errors: Partial<Record<keyof IdpFormValues, string>> = {};
	if (!values.name || values.name.length < 2) errors.name = "Name too short";
	if (!values.issuer || !values.issuer.startsWith("http")) errors.issuer = "Must be a valid URL";
	if (!values.clientId) errors.clientId = "Client ID required";
	return errors;
}

function IdpModal({
	opened,
	onClose,
	idp,
}: {
	opened: boolean;
	onClose: () => void;
	idp: IdentityProviderResource | null;
}) {
	const create = useCreateIdentityProvider();
	const update = useUpdateIdentityProvider();
	const isEditing = !!idp;

	const initialValues: IdpFormValues = idp
		? {
				name: idp.name,
				title: idp.title ?? "",
				description: idp.description ?? "",
				type: idp.type ?? "oidc",
				issuer: idp.issuer,
				clientId: idp.clientId,
				clientSecret: "",
				authorizeUrl: idp.authorizeUrl ?? "",
				tokenUrl: idp.tokenUrl ?? "",
				jwksUrl: idp.jwksUrl ?? "",
				userInfoUrl: idp.userInfoUrl ?? "",
				scopes: idp.scopes ?? ["openid", "profile", "email"],
				active: idp.active,
			}
		: IDP_DEFAULTS;

	const handleSubmit = async (values: IdpFormValues) => {
		const payload: IdentityProviderResource = {
			resourceType: "IdentityProvider",
			...values,
		};
		if (isEditing && !values.clientSecret) delete payload.clientSecret;
		try {
			if (isEditing && idp?.id) {
				await update.mutateAsync({ ...payload, id: idp.id });
			} else {
				await create.mutateAsync(payload);
			}
			onClose();
		} catch {
			/* surfaced by mutation */
		}
	};

	return (
		<Modal
			opened={opened}
			onClose={onClose}
			title={isEditing ? "Edit Identity Provider" : "Add Identity Provider"}
			size="lg"
		>
			<Form<IdpFormValues>
				key={idp?.id ?? "new"}
				onSubmit={handleSubmit}
				validate={validateIdp}
				initialValues={initialValues}
				render={({ handleSubmit: submit, submitting }) => (
					<form onSubmit={submit}>
						<div className={classes.idpForm}>
							<div className={classes.formGrid}>
								<Field<string> name="name">
									{({ input, meta }) => (
										<TextInput
											label="Internal Name"
											required
											value={input.value}
											onChange={input.onChange}
											onBlur={input.onBlur}
											error={meta.touched && meta.error ? meta.error : undefined}
										/>
									)}
								</Field>
								<Field<string> name="title">
									{({ input }) => (
										<TextInput
											label="Display Title"
											value={input.value}
											onChange={input.onChange}
										/>
									)}
								</Field>
							</div>

							<Field<string> name="type">
								{({ input }) => (
									<Select
										label="Type"
										data={identityProviderTypeOptions}
										value={input.value}
										onChange={input.onChange}
									/>
								)}
							</Field>

							<Field<string> name="issuer">
								{({ input, meta }) => (
									<TextInput
										label="Issuer URL"
										required
										value={input.value}
										onChange={input.onChange}
										onBlur={input.onBlur}
										error={meta.touched && meta.error ? meta.error : undefined}
									/>
								)}
							</Field>

							<div className={classes.formGrid}>
								<Field<string> name="clientId">
									{({ input, meta }) => (
										<TextInput
											label="Client ID"
											required
											value={input.value}
											onChange={input.onChange}
											onBlur={input.onBlur}
											error={meta.touched && meta.error ? meta.error : undefined}
										/>
									)}
								</Field>
								<Field<string> name="clientSecret">
									{({ input }) => (
										<PasswordInput
											label="Client Secret"
											placeholder={isEditing ? "Leave blank to keep current" : ""}
											value={input.value}
											onChange={input.onChange}
										/>
									)}
								</Field>
							</div>

							<div className={classes.formGrid}>
								<Field<string> name="authorizeUrl">
									{({ input }) => (
										<TextInput label="Authorize URL" value={input.value} onChange={input.onChange} />
									)}
								</Field>
								<Field<string> name="tokenUrl">
									{({ input }) => (
										<TextInput label="Token URL" value={input.value} onChange={input.onChange} />
									)}
								</Field>
							</div>

							<div className={classes.formGrid}>
								<Field<string> name="jwksUrl">
									{({ input }) => (
										<TextInput label="JWKS URL" value={input.value} onChange={input.onChange} />
									)}
								</Field>
								<Field<string> name="userInfoUrl">
									{({ input }) => (
										<TextInput label="User Info URL" value={input.value} onChange={input.onChange} />
									)}
								</Field>
							</div>

							<Field<string[]> name="scopes">
								{({ input }) => (
									<MultiSelect
										label="Default Scopes"
										data={input.value}
										searchable
										value={input.value}
										onChange={input.onChange}
									/>
								)}
							</Field>

							<Field<boolean> name="active" type="checkbox">
								{({ input }) => (
									<Switch content="Active" checked={input.checked ?? false} onUpdate={input.onChange} />
								)}
							</Field>

							<div className={classes.formActions}>
								<Button variant="subtle" onClick={onClose} type="button">
									Cancel
								</Button>
								<Button
									variant="filled"
									type="submit"
									loading={submitting || create.isPending || update.isPending}
								>
									{isEditing ? "Update" : "Create"}
								</Button>
							</div>
						</div>
					</form>
				)}
			/>
		</Modal>
	);
}
