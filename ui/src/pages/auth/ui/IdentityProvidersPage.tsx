import {
  Alert,
  Badge,
  Button,
  Card,
  DataPreview,
  DropdownMenu,
  EmptyState,
  Field,
  Form,
  Modal,
  MultiSelect,
  PasswordInput,
  Skeleton,
  Switch,
  Text,
  TextInput,
  useDebouncedValue,
  useDisclosure,
  type DropdownMenuItems,
} from "@octofhir/ui-kit";
import {
  EllipsisVertical,
  Globe,
  Search as Magnifier,
  Pencil,
  Plus,
  Trash2 as TrashBin,
} from "lucide-react";
import { useState } from "react";
import {
  getIdentityProviderStatusView,
  getIdentityProviderTypeView,
  type ClaimToRoles,
  type IdentityProviderResource,
} from "@/entities/identity-provider";
import { getBundleResources } from "@/shared/api/guards";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import {
  useCreateIdentityProvider,
  useDeleteIdentityProvider,
  useIdentityProviders,
  useUpdateIdentityProvider,
} from "../lib/useIdentityProviders";
import classes from "./IdentityProvidersPage.module.css";

export function IdentityProvidersPage() {
  const [search, setSearch] = useState("");
  const debouncedSearch = useDebouncedValue(search, 500);
  const [opened, { open, close }] = useDisclosure(false);
  const [editingIdp, setEditingIdp] = useState<IdentityProviderResource | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<IdentityProviderResource | null>(null);

  const { data, isLoading, isError, error, refetch } = useIdentityProviders({
    search: debouncedSearch,
  });
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

  const providerActions = (provider: (typeof providers)[number]): DropdownMenuItems => [
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
  ];

  return (
    <WorkspacePageLayout
      title="Identity providers"
      description="OpenID Connect providers people can sign in through"
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
            description={
              error instanceof Error
                ? error.message
                : "Something went wrong while loading identity providers."
            }
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
                    <Globe
                      width={16}
                      height={16}
                      className={classes.providerIcon}
                      aria-hidden="true"
                    />
                    <div className={classes.providerText}>
                      <Text variant="body-2" className={classes.providerName}>
                        <strong>{provider.name}</strong>
                      </Text>
                      <Text
                        variant="caption-2"
                        color="secondary"
                        className={classes.providerIssuer}
                      >
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
                    items={providerActions(provider)}
                  />
                ),
              };
            })}
            getRowMenu={(_row, index) => providerActions(providers[index])}
            getRowKey={(_row, index) =>
              providers[index]?.id ?? providers[index]?.name ?? `${index}`
            }
          />
        )}
      </Card>

      <IdpModal opened={opened} onClose={handleClose} idp={editingIdp} />

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

interface ClaimRule {
  claim: string;
  value: string;
  roles: string;
}

interface IdpFormValues {
  name: string;
  displayName: string;
  logoUrl: string;
  issuer: string;
  clientId: string;
  clientSecret: string;
  authorizationEndpoint: string;
  tokenEndpoint: string;
  userinfoEndpoint: string;
  scopes: string[];
  active: boolean;
  rules: ClaimRule[];
}

const IDP_DEFAULTS: IdpFormValues = {
  name: "",
  displayName: "",
  logoUrl: "",
  issuer: "",
  clientId: "",
  clientSecret: "",
  authorizationEndpoint: "",
  tokenEndpoint: "",
  userinfoEndpoint: "",
  scopes: ["openid", "profile", "email"],
  active: true,
  rules: [],
};

const toRules = (rules: ClaimToRoles[] = []): ClaimRule[] =>
  rules.map((rule) => ({ ...rule, roles: rule.roles.join(", ") }));

const fromRules = (rules: ClaimRule[]): ClaimToRoles[] =>
  rules
    .filter((rule) => rule.claim && rule.value && rule.roles)
    .map((rule) => ({
      claim: rule.claim.trim(),
      value: rule.value.trim(),
      roles: rule.roles
        .split(",")
        .map((role) => role.trim())
        .filter(Boolean),
    }));

/** Rules that turn the groups a provider reports into roles. */
function ClaimRules({
  value,
  onChange,
}: {
  value: ClaimRule[];
  onChange: (rules: ClaimRule[]) => void;
}) {
  const edit = (index: number, change: Partial<ClaimRule>) =>
    onChange(value.map((rule, at) => (at === index ? { ...rule, ...change } : rule)));
  return (
    <div className={classes.idpForm}>
      <Text variant="caption-2" color="secondary">
        With rules the provider decides roles at every sign-in, and nobody signs in without one.
        Value: exact, with *, or ^regex$. $1 in a role stands for what was caught.
      </Text>
      {value.map((rule, index) => (
        <div key={index} className={classes.ruleRow}>
          <TextInput
            placeholder="groups"
            aria-label="Claim"
            value={rule.claim}
            onChange={(claim) => edit(index, { claim })}
          />
          <TextInput
            placeholder="fhir-*"
            aria-label="Value"
            value={rule.value}
            onChange={(value) => edit(index, { value })}
          />
          <TextInput
            placeholder="$1, reader"
            aria-label="Roles"
            value={rule.roles}
            onChange={(roles) => edit(index, { roles })}
          />
          <Button
            variant="subtle"
            type="button"
            aria-label="Remove rule"
            onClick={() => onChange(value.filter((_, at) => at !== index))}
          >
            <TrashBin size={14} />
          </Button>
        </div>
      ))}
      <div>
        <Button
          variant="outline"
          type="button"
          onClick={() => onChange([...value, { claim: "groups", value: "", roles: "" }])}
        >
          Add rule
        </Button>
      </div>
    </div>
  );
}

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
        displayName: idp.displayName ?? "",
        logoUrl: idp.logoUrl ?? "",
        issuer: idp.issuer,
        clientId: idp.clientId,
        clientSecret: "",
        authorizationEndpoint: idp.authorizationEndpoint ?? "",
        tokenEndpoint: idp.tokenEndpoint ?? "",
        userinfoEndpoint: idp.userinfoEndpoint ?? "",
        scopes: idp.scopes ?? ["openid", "profile", "email"],
        active: idp.active,
        rules: toRules(idp.userMapping?.claimsToRoles),
      }
    : IDP_DEFAULTS;

  const handleSubmit = async ({ rules, ...values }: IdpFormValues) => {
    // An element the resource does not use is left out, not sent empty.
    const filled = Object.fromEntries(
      Object.entries(values).filter(([, value]) => value !== ""),
    ) as Pick<IdentityProviderResource, "name" | "issuer" | "clientId" | "active" | "scopes">;
    const claimsToRoles = fromRules(rules);
    const { claimsToRoles: _replaced, ...mapping } = idp?.userMapping ?? {};
    const userMapping = claimsToRoles.length > 0 ? { ...mapping, claimsToRoles } : mapping;
    const payload: IdentityProviderResource = {
      resourceType: "IdentityProvider",
      type: "oidc",
      ...filled,
      ...(Object.keys(userMapping).length > 0 ? { userMapping } : {}),
    };
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
                <Field<string> name="displayName">
                  {({ input }) => (
                    <TextInput
                      label="Button Label"
                      placeholder="Google"
                      value={input.value}
                      onChange={input.onChange}
                    />
                  )}
                </Field>
              </div>

              <Field<string> name="logoUrl">
                {({ input }) => (
                  <TextInput
                    label="Logo URL"
                    placeholder="https://example.org/logo.svg"
                    value={input.value}
                    onChange={input.onChange}
                  />
                )}
              </Field>

              <Text variant="caption-2" color="secondary">
                Redirect URI to register at the provider: {window.location.origin}
                /auth/idp/callback
              </Text>

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
                <Field<string> name="authorizationEndpoint">
                  {({ input }) => (
                    <TextInput
                      label="Authorization Endpoint"
                      placeholder="From discovery"
                      value={input.value}
                      onChange={input.onChange}
                    />
                  )}
                </Field>
                <Field<string> name="tokenEndpoint">
                  {({ input }) => (
                    <TextInput
                      label="Token Endpoint"
                      placeholder="From discovery"
                      value={input.value}
                      onChange={input.onChange}
                    />
                  )}
                </Field>
              </div>

              <Field<string> name="userinfoEndpoint">
                {({ input }) => (
                  <TextInput
                    label="Userinfo Endpoint"
                    placeholder="From discovery"
                    value={input.value}
                    onChange={input.onChange}
                  />
                )}
              </Field>

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

              <Field<ClaimRule[]> name="rules">
                {({ input }) => <ClaimRules value={input.value || []} onChange={input.onChange} />}
              </Field>

              <Field<boolean> name="active" type="checkbox">
                {({ input }) => (
                  <Switch
                    content="Active"
                    checked={input.checked ?? false}
                    onUpdate={input.onChange}
                  />
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
