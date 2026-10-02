# Publisher Links: A Guide For Feed Authors

This guide tells a feed author and a feed tool how to link music albums to the
artists and labels that release them. It also tells how musicindex.org reads
the links. It is advisory. The rules belong to
[ADR 0049](adr/0049-publisher-relationships-are-rss-facts.md),
[ADR 0061](adr/0061-a-publisher-read-counts-its-listed-artists.md) and
[ADR 0068](adr/0068-a-publisher-row-gives-its-link-facts.md). The credit form
of this guide is
[ADR 0069](adr/0069-an-album-confirms-each-credit.md).

`rel` on `<podcast:remoteItem>` is not in the Podcast Namespace yet. It is
proposed in
[discussion #579](https://github.com/Podcastindex-org/podcast-namespace/discussions/579).
An app that does not know the attribute ignores it.

## The Rule: Only A Two-Way Link Is True

A publisher feed (`<podcast:medium>publisher</podcast:medium>`) lists feeds
with `<podcast:remoteItem>`. Any publisher feed can list any album. So a
listing alone is a claim.

A link is true when both feeds state it: the publisher feed lists the album,
and the album names the publisher feed. The namespace says the same in its
[publisher documentation](https://github.com/Podcastindex-org/podcast-namespace/blob/main/docs/examples/publishers/publishers.md):
a link that the other side does not confirm "should be discarded".

musicindex.org shows a one-way link as a claim, and never counts it as a
fact.

## The Parts

| Part | Where | What it says |
|---|---|---|
| The publisher | The `remoteItem` inside `<podcast:publisher>` of the album | "This feed releases me." One only |
| A credit | A `remoteItem` with `medium="publisher"` directly in `<channel>` of the album | "This feed also takes part in me." Zero or more |
| A listing | A `remoteItem` with `medium="music"` in a publisher feed | "I take part in this album." |
| The role | `rel` on each of these items | `artist` or `label`. Other values are allowed, for example `producer` |

Each link has two items: one in the album, one in the publisher feed. Write the
same `rel` value on both.

When `rel` is absent, the role is not stated. An app must not show a guess
as a fact.

## Example 1: An Artist Releases An Album

The album names the artist feed as its publisher. The artist feed lists the
album.

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

Result: one true link. The artist feed is the artist of the album.

## Example 2: A Label Releases An Album

The album names the label as its publisher, and credits the artist. The label
feed and the artist feed each list the album.

```xml
<!-- The album -->
<podcast:publisher>
    <podcast:remoteItem medium="publisher" rel="label"
        feedGuid="LABEL-FEED-GUID" feedUrl="https://example.com/label.xml" />
</podcast:publisher>
<podcast:remoteItem medium="publisher" rel="artist"
    feedGuid="ARTIST-FEED-GUID" feedUrl="https://example.com/artist.xml" />

<!-- The label feed -->
<podcast:remoteItem medium="music" rel="label"
    feedGuid="ALBUM-GUID" feedUrl="https://example.com/album.xml" />

<!-- The artist feed -->
<podcast:remoteItem medium="music" rel="artist"
    feedGuid="ALBUM-GUID" feedUrl="https://example.com/album.xml" />
```

Result: two true links. An app that knows only `<podcast:publisher>` sees the
label, as before. musicindex.org also shows the artist, and it lists the
artist on the label page, and the label on the artist page.

## Example 3: Two Artists Release An Album Together

The album names one artist feed as its publisher and credits the other. Each
artist feed lists the album with `rel="artist"`.

## What Each Side Controls

| Who | Can do | Cannot do |
|---|---|---|
| The album | Name its publisher, and credit any feed | Make a link true alone |
| A publisher feed | List any album | Make a link true alone |
| A label | List its releases | Add an artist to an album, or take an album that does not name it |

Only the album and the publisher feed together make a link true. A label
cannot state the artist of an album. The album states its own artist, with
`itunes:author` and `podcast:person`, and with a credit.

## Notes For A Feed Tool

1. Write the release owner in `<podcast:publisher>`: the label for a label
   release, the artist for a release by the artist.
2. Write each other party as a channel `remoteItem` with `medium="publisher"`
   and a `rel`.
3. Keep each channel `remoteItem` with `medium="publisher"` when you write the
   feed again. Do not merge it into `<podcast:publisher>`.
4. When the album has no `<podcast:publisher>`, an app reads the first channel
   `remoteItem` with `medium="publisher"` as the publisher. So always write
   `<podcast:publisher>` when you also write a credit.
5. When the user changes a role, write the new `rel` on both sides of the link.
   A tool that cannot change the other feed, because another host keeps it,
   should tell the user to change that feed.
6. Write nothing for "role not stated". Do not write an empty `rel`.
7. Send a podping for each feed that changed.

## How musicindex.org Reads A Link

The node at `api.musicindex.org` gives each link in the `publisher` view of a
feed read (`GET /v1/feeds/{guid}?include=publisher`):

| Field | Meaning |
|---|---|
| `two_way_validated` | Both feeds state the link |
| `publisher_rel`, `music_rel` | The raw `rel` of each side |
| `role`, `role_source` | The stated role, and which side stated it. `role` is null when no side states one (`default`) or when the sides differ (`conflict`) |
| `album_names_as` | `publisher`, `credit`, or null. ADR 0069, from release 0.6.0 |

A publisher read also gives `confirmed_release_artists`, the artists of its
true links, and `unconfirmed_release_artists`, the artists of its one-way
listings. From release 0.6.0 it also gives `co_credited_feeds`, the other publisher
feeds of its true albums.
