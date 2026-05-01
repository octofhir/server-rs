import type React from 'react';

const LAYOUT_PROP_KEYS = [
    'w',
    'h',
    'p',
    'px',
    'py',
    'pt',
    'pb',
    'pl',
    'pr',
    'm',
    'mx',
    'my',
    'mt',
    'mb',
    'ml',
    'mr',
] as const;

type LayoutPropKey = (typeof LAYOUT_PROP_KEYS)[number];

export const mapSpaceValue = (val?: number | string): number | string | undefined => {
    if (typeof val === 'number') return val;
    switch(val) {
        case 'xs': return 8;
        case 'sm': return 12;
        case 'md': return 16;
        case 'lg': return 24;
        case 'xl': return 32;
        case 'auto': return 'auto';
        default: return val;
    }
}

export const getSpacingStyles = (props: any): React.CSSProperties => {
    const { w, h, p, px, py, pt, pb, pl, pr, m, mx, my, mt, mb, ml, mr } = props;
    return {
        ...(w !== undefined ? { width: mapSpaceValue(w) } : {}),
        ...(h !== undefined ? { height: mapSpaceValue(h) } : {}),
        ...(p !== undefined ? { padding: mapSpaceValue(p) } : {}),
        ...(px !== undefined ? { paddingLeft: mapSpaceValue(px), paddingRight: mapSpaceValue(px) } : {}),
        ...(py !== undefined ? { paddingTop: mapSpaceValue(py), paddingBottom: mapSpaceValue(py) } : {}),
        ...(pt !== undefined ? { paddingTop: mapSpaceValue(pt) } : {}),
        ...(pb !== undefined ? { paddingBottom: mapSpaceValue(pb) } : {}),
        ...(pl !== undefined ? { paddingLeft: mapSpaceValue(pl) } : {}),
        ...(pr !== undefined ? { paddingRight: mapSpaceValue(pr) } : {}),
        ...(m !== undefined ? { margin: mapSpaceValue(m) } : {}),
        ...(mx !== undefined ? { marginLeft: mapSpaceValue(mx), marginRight: mapSpaceValue(mx) } : {}),
        ...(my !== undefined ? { marginTop: mapSpaceValue(my), marginBottom: mapSpaceValue(my) } : {}),
        ...(mt !== undefined ? { marginTop: mapSpaceValue(mt) } : {}),
        ...(mb !== undefined ? { marginBottom: mapSpaceValue(mb) } : {}),
        ...(ml !== undefined ? { marginLeft: mapSpaceValue(ml) } : {}),
        ...(mr !== undefined ? { marginRight: mapSpaceValue(mr) } : {}),
    };
};

export const cleanLayoutProps = <T extends Record<string, any>>(
    props: T,
): Omit<T, LayoutPropKey> => {
    const rest = { ...props };
    for (const key of LAYOUT_PROP_KEYS) {
        delete rest[key];
    }
    return rest;
};
