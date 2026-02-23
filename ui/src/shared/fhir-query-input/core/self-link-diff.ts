export interface SelfLinkDiff {
	added: string[];
	removed: string[];
	modified: Array<{ param: string; sent: string; received: string }>;
	identical: string[];
}

export function diffSelfLink(sentUrl: string, selfLink: string): SelfLinkDiff {
	const sentParams = parseParams(sentUrl);
	const selfParams = parseParams(selfLink);

	const added: string[] = [];
	const removed: string[] = [];
	const modified: Array<{ param: string; sent: string; received: string }> = [];
	const identical: string[] = [];

	const allKeys = new Set([...sentParams.keys(), ...selfParams.keys()]);

	for (const key of allKeys) {
		const sentValue = sentParams.get(key);
		const selfValue = selfParams.get(key);

		if (sentValue === undefined && selfValue !== undefined) {
			added.push(`${key}=${selfValue}`);
		} else if (sentValue !== undefined && selfValue === undefined) {
			removed.push(key);
		} else if (sentValue !== selfValue) {
			modified.push({
				param: key,
				sent: sentValue ?? "",
				received: selfValue ?? "",
			});
		} else {
			identical.push(key);
		}
	}

	return { added, removed, modified, identical };
}

function parseParams(url: string): Map<string, string> {
	const params = new Map<string, string>();
	const qIdx = url.indexOf("?");
	if (qIdx === -1) return params;

	const queryString = url.slice(qIdx + 1);
	for (const pair of queryString.split("&")) {
		const eqIdx = pair.indexOf("=");
		if (eqIdx === -1) {
			params.set(pair, "");
		} else {
			params.set(pair.slice(0, eqIdx), pair.slice(eqIdx + 1));
		}
	}
	return params;
}
