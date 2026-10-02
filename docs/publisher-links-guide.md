# Publisher Links: A Guide For Feed Authors

This guide tells a feed author and a feed tool how to link music albums to the
artists and labels that release them. It also tells how musicindex.org reads
the links. It is advisory. The rules belong to
[ADR 0049](adr/0049-publisher-relationships-are-rss-facts.md),
[ADR 0061](adr/0061-a-publisher-read-counts-its-listed-artists.md),
[ADR 0068](adr/0068-a-publisher-row-gives-its-link-facts.md) and
[ADR 0069](adr/0069-an-album-confirms-each-credit.md).

The form of this guide follows
[podcast-namespace PR #793](https://github.com/Podcastindex-org/podcast-namespace/pull/793).
The PR is open on 2026-10-02. It adds `rel` to `<podcast:remoteItem>`, and it
lets `<podcast:publisher>` hold one item for each party. Until the PR is
merged, the specification says that `<podcast:publisher>` holds exactly one
item. An app that does not know `rel` ignores it.

## The Rule: Only A Two-Way Link Is True

A publisher feed (`<podcast:medium>publisher</podcast:medium>`) lists feeds
with `<podcast:remoteItem>`. Any publisher feed can list any album. So a
listing alone is a claim.

A link is true when both feeds state it: the publisher feed lists the album,
and the album names the publisher feed. The namespace says the same in its
[publisher documentation](https://github.com/Podcastindex-org/podcast-namespace/blob/main/docs/examples/publishers/publishers.md):
a link that the other side does not confirm "should be discarded".

## The Parts

| Part | Where | What it says |
|---|---|---|
| A publisher | A `remoteItem` with `medium="publisher"` inside `<podcast:publisher>` of the album. One item for each party, the primary party first | "This feed takes part in me." |
| A listing | A `remoteItem` with `medium="music"` in a publisher feed | "I take part in this album." |
| The role | `rel` on each of these items | A set of tokens, separated by spaces |

PR #793 gives these starting tokens: `artist`, `host`, `author`, `label`,
`producer`, `network`, `hosting` and `sponsor`. One party with two roles keeps
one item, with both tokens: `rel="artist producer"`. A second item with the
same `feedGuid` looks like a second party.

Each link has two items: one in the album, one in the publisher feed. Write the
same `rel` set on both. The order of the tokens does not matter.

When `rel` is absent, the link means only "this feed takes part". The role is
not stated. An app must not show a guess as a fact.

## Example 1: An Artist Releases An Album

The album names the artist feed. The artist feed lists the album.

```xml
<!-- The album -->
<podcast:publisher>
    <podcast:remoteItem medium="publisher" rel="artist"
        feedGuid="ARTIST-FEED-GUID" feedUrl="https://example.com/artist.xml" />
</podcast:publisher>

<!-- The artist feed -->
<podcast:remoteItem medium="music" rel="artist"
    feedGuid="ALBUM-GUID" feedUrl="https://example.com/album.xml" />
```

Result: one true link, with the role `artist` on both sides.

## Example 2: A Label Releases An Album

The album names the artist and the label. The artist feed and the label feed
each list the album.

```xml
<!-- The album -->
<podcast:publisher>
    <podcast:remoteItem medium="publisher" rel="artist"
        feedGuid="ARTIST-FEED-GUID" feedUrl="https://example.com/artist.xml" />
    <podcast:remoteItem medium="publisher" rel="label"
        feedGuid="LABEL-FEED-GUID" feedUrl="https://example.com/label.xml" />
</podcast:publisher>

<!-- The artist feed -->
<podcast:remoteItem medium="music" rel="artist"
    feedGuid="ALBUM-GUID" feedUrl="https://example.com/album.xml" />

<!-- The label feed -->
<podcast:remoteItem medium="music" rel="label"
    feedGuid="ALBUM-GUID" feedUrl="https://example.com/album.xml" />
```

Result: two true links. An app that reads only the first item sees the primary
party, here the artist. musicindex.org shows both, and it lists the artist on
the label page, and the label on the artist page.

## Example 3: Two Artists Release An Album Together

The album names both artist feeds inside `<podcast:publisher>`, each with
`rel="artist"`. Each artist feed lists the album with `rel="artist"`.

## Example 4: One Party With Two Roles

Jimmy V is the artist and the producer of an album. The album names his feed
once, and his feed lists the album once, each with `rel="artist producer"`.
`rel="producer artist"` agrees too.

## What Each Side Controls

| Who | Can do | Cannot do |
|---|---|---|
| The album | Name each party | Make a link true alone |
| A publisher feed | List any album | Make a link true alone |
| A label | List its releases | Add an artist to an album, or take an album that does not name it |

Only the album and the publisher feed together make a link true. A label
cannot state the artist of an album. The album states its own artist, with
`itunes:author`, `podcast:person`, and the parties that it names.

## Notes For A Feed Tool

1. Write one `remoteItem` with `medium="publisher"` for each party inside one
   `<podcast:publisher>`. Write the primary party first: the artist for a
   release by the artist.
2. Keep every item inside `<podcast:publisher>` when you write the feed again.
   A tool that keeps only the first item deletes the other parties.
3. When your tool adds its own publisher to an album, add or update that one
   item. Do not replace the other items.
4. When the user changes a role, write the new `rel` on both sides of the link.
   A tool that cannot change the other feed, because another host keeps it,
   should tell the user to change that feed.
5. Write nothing for "role not stated". Do not write an empty `rel`.
6. Send a podping for each feed that changed.

## How musicindex.org Reads A Link

The node at `api.musicindex.org` follows the link rules of PR #793. It gives
each link in the `publisher` view of a feed read
(`GET /v1/feeds/{guid}?include=publisher`):

| Field | Meaning |
|---|---|
| `two_way_validated` | Both feeds state the link |
| `publisher_rel`, `music_rel` | The raw `rel` of each side |
| `role`, `role_source` | The role set, and which side stated it. `role` is null when no side states one (`default`), and when the two sides state different sets (`conflict`) |
| `role_agreement` | `both`: the two sides state the same set. `one_side`: only one side states a role. `conflict`: the sets differ. Null: no side states a role |
| `album_names_as` | `publisher` when the album names the feed inside `<podcast:publisher>`. Null when the album does not name the feed |

A confirmed link is a two-way link with a `role_agreement` that is not
`conflict`.
This is the "discard" rule of PR #793. A link with a `conflict` stays in the
view with each raw value, so a feed author can find it and correct it. But no
count uses it.

A role that only one side states is not an agreed role. The node shows it as
`role`, with `role_agreement` `one_side`. For a client that follows PR #793
fully, the link is a plain publisher link.

The node reads a `rel` value with a comma as a list separated by commas, and
a value with no comma as a list separated by spaces.

A publisher read also gives `confirmed_release_artists`, the artists of its
confirmed links. It gives `unconfirmed_release_artists`, the artists of its
other listings. It also gives `co_credited_feeds`. This list holds the other
publisher feeds that its confirmed albums name in a confirmed link.

The publisher list, `GET /v1/feeds/recent?medium=publisher&include=link_facts`,
gives the role tokens of each publisher. `stated_rels` holds each token of its
confirmed links. `agreed_roles` holds each token that the two sides state.
`stated_rel=label` keeps the publishers with the token `label`.

### Older Forms

An album with no `<podcast:publisher>` can name its publisher with a bare
`remoteItem` with `medium="publisher"` directly in `<channel>`. The node reads
the first such item as the publisher. It does not read each other bare item
as a link. Write the form of PR #793 in a new feed.
