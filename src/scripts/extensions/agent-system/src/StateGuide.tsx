/**
 * The block an editor opens with.
 *
 * A form answers "which key goes in this box" and never "what is this for": a
 * first-time reader cannot tell what a declaration, a machine or an access grid
 * is supposed to do, or how the three relate. One short block, kept where the
 * concepts live, said in terms of what a thing does rather than how it is
 * stored.
 */

export function StateGuide({
    title,
    lead,
    points = [],
}: {
    title: string;
    lead: string;
    points?: readonly string[];
}) {
    return (
        <div className="ttas-state-guide">
            <div className="ttas-section-title">
                <i className="fa-solid fa-circle-info"></i>
                <h4>{title}</h4>
            </div>
            <p className="ttas-state-guide-lead">{lead}</p>
            {points.length > 0 && (
                <ul className="ttas-state-guide-points">
                    {points.map((point, index) => <li key={index}>{point}</li>)}
                </ul>
            )}
        </div>
    );
}
