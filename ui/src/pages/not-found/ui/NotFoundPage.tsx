import { Button, Kbd } from "@octofhir/ui-kit";
import { ArrowLeft, ArrowRight, House, Search } from "lucide-react";
import { useMemo } from "react";
import { Helmet } from "react-helmet-async";
import { useLocation, useNavigate } from "react-router-dom";
import { paletteOpenChanged } from "@/shared/command-palette";
import { NAV_PAGES } from "@/widgets/app-shell";
import { closestPages, resourceAddress } from "../lib/suggest";
import classes from "./NotFoundPage.module.css";

/**
 * Shown inside the shell for any console address without a page. It offers what the address
 * most likely meant: a FHIR resource (`/Patient/123`) or sidebar pages with a similar name.
 */
export function NotFoundPage() {
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const resource = useMemo(() => resourceAddress(pathname), [pathname]);
  const suggestions = useMemo(() => closestPages(pathname, NAV_PAGES), [pathname]);
  // React Router keeps its own history index; 0 means this tab opened on the missing page.
  const canGoBack = ((window.history.state as { idx?: number } | null)?.idx ?? 0) > 0;

  return (
    <div className={classes.root}>
      <Helmet>
        <title>Not found · OctoFHIR</title>
      </Helmet>
      <div className={classes.card}>
        <div className={classes.code} aria-hidden="true">
          <span className={classes.slash}>/</span>404
        </div>
        <h1 className={classes.title}>No page at this address</h1>
        <code className={classes.path}>{pathname}</code>

        {resource && (
          <button type="button" className={classes.row} onClick={() => navigate(resource)}>
            <span className={classes.rowLabel}>Open in resource browser</span>
            <span className={classes.rowPath}>{resource.replace("/resources/", "")}</span>
            <ArrowRight size={14} className={classes.rowArrow} aria-hidden="true" />
          </button>
        )}

        {suggestions.length > 0 && (
          <nav className={classes.suggestions} aria-label="Similar pages">
            <span className={classes.label}>Did you mean</span>
            {suggestions.map((page) => (
              <button key={page.path} type="button" className={classes.row} onClick={() => navigate(page.path)}>
                <span className={classes.rowLabel}>{page.label}</span>
                <span className={classes.rowSection}>{page.section}</span>
                <span className={classes.rowPath}>{page.path}</span>
                <ArrowRight size={14} className={classes.rowArrow} aria-hidden="true" />
              </button>
            ))}
          </nav>
        )}

        <div className={classes.actions}>
          {canGoBack && (
            <Button variant="subtle" onClick={() => navigate(-1)}>
              <Button.Icon side="left">
                <ArrowLeft size={14} />
              </Button.Icon>
              Back
            </Button>
          )}
          <Button variant="outline" onClick={() => paletteOpenChanged(true)}>
            <Button.Icon side="left">
              <Search size={14} />
            </Button.Icon>
            Search pages
            <Kbd size="xs" value="mod+k" className={classes.kbd} />
          </Button>
          <Button variant={resource || suggestions.length ? "outline" : "filled"} onClick={() => navigate("/")}>
            <Button.Icon side="left">
              <House size={14} />
            </Button.Icon>
            Overview
          </Button>
        </div>
      </div>
    </div>
  );
}
