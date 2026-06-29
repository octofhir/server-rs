import "@mdxeditor/editor/style.css";
import {
  BlockTypeSelect,
  BoldItalicUnderlineToggles,
  CreateLink,
  codeBlockPlugin,
  codeMirrorPlugin,
  headingsPlugin,
  InsertCodeBlock,
  InsertTable,
  InsertThematicBreak,
  ListsToggle,
  linkDialogPlugin,
  linkPlugin,
  listsPlugin,
  MDXEditor,
  type MDXEditorMethods,
  markdownShortcutPlugin,
  quotePlugin,
  Separator,
  tablePlugin,
  thematicBreakPlugin,
  toolbarPlugin,
  UndoRedo,
} from "@mdxeditor/editor";
import { useColorScheme } from "@octofhir/ui-kit";
import { useEffect, useRef } from "react";
import classes from "./MarkdownCellEditor.module.css";

interface Props {
  value: string;
  onChange: (md: string) => void;
}

/**
 * Modern WYSIWYG markdown editor for notebook prose cells. Single editing surface
 * (not split preview) with a flat toolbar. Backed by @mdxeditor/editor.
 */
export function MarkdownCellEditor({ value, onChange }: Props) {
  const { colorScheme } = useColorScheme();
  const ref = useRef<MDXEditorMethods>(null);

  // Keep the editor in sync when the value is replaced externally (e.g. cell
  // type switch, notebook load) without clobbering in-flight typing.
  useEffect(() => {
    if (ref.current && ref.current.getMarkdown() !== value) {
      ref.current.setMarkdown(value);
    }
  }, [value]);

  return (
    <div className={`${classes.wrap} ${colorScheme === "dark" ? "dark-theme" : ""}`}>
      <MDXEditor
        ref={ref}
        markdown={value}
        onChange={onChange}
        contentEditableClassName={classes.content}
        plugins={[
          headingsPlugin(),
          listsPlugin(),
          quotePlugin(),
          thematicBreakPlugin(),
          linkPlugin(),
          linkDialogPlugin(),
          tablePlugin(),
          codeBlockPlugin({ defaultCodeBlockLanguage: "ts" }),
          codeMirrorPlugin({
            codeBlockLanguages: {
              ts: "TypeScript",
              js: "JavaScript",
              sql: "SQL",
              json: "JSON",
              text: "Plain text",
            },
          }),
          markdownShortcutPlugin(),
          toolbarPlugin({
            toolbarContents: () => (
              <>
                <UndoRedo />
                <Separator />
                <BoldItalicUnderlineToggles />
                <Separator />
                <BlockTypeSelect />
                <Separator />
                <ListsToggle />
                <CreateLink />
                <InsertTable />
                <InsertCodeBlock />
                <InsertThematicBreak />
              </>
            ),
          }),
        ]}
      />
    </div>
  );
}
