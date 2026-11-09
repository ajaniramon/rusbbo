{% set title = locale.hk_noticias %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <input type="search" data-filter="news-table" placeholder="{{ locale.hk_filtrar_noticias|raw }}">
  <span class="spacer"></span>
  <a class="btn primary" href="{{ hk.path }}/articles/create">{{ locale.hk_nueva_noticia|raw }}</a>
</div>
<div class="card flush">
  <div class="card-body scroll">
    {% if articles is empty %}
      <div class="empty">{{ locale.hk_todavia_no_hay_noticias_las_noticias_publicadas|raw }}</div>
    {% else %}
    <table class="tbl" id="news-table">
      <thead><tr><th>{{ locale.hk_titulo|raw }}</th><th>{{ locale.hk_autor|raw }}</th><th>{{ locale.hk_fecha|raw }}</th><th>{{ locale.hk_estado|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for a in articles %}
        <tr>
          <td><strong>{{ a.getTitle() }}</strong><div class="muted small">{{ a.getShortStory() }}</div></td>
          <td>{{ a.getAuthor() }}</td>
          <td class="nowrap muted">{{ a.getDate() }}</td>
          <td>{% if a.isPublished() %}<span class="pill ok">{{ locale.hk_publicada|raw }}</span>{% elseif a.isFuturePublished() %}<span class="pill info">{{ locale.hk_programada|raw }}</span>{% else %}<span class="pill">{{ locale.hk_borrador|raw }}</span>{% endif %}</td>
          <td class="actions">
            <a class="btn sm" href="/articles/{{ a.getUrl() }}" target="_blank" rel="noopener">{{ locale.hk_ver|raw }}</a>
            <a class="btn sm" href="{{ hk.path }}/articles/edit?id={{ a.getId() }}">{{ locale.hk_editar|raw }}</a>
            <form class="inline" method="post" action="{{ hk.path }}/articles/delete?id={{ a.getId() }}" data-confirm="{{ locale.hk_borrar_la_noticia|raw }}{{ a.getTitle() }}»?">
              <input type="hidden" name="hk_token" value="{{ hk.token }}"><button class="btn sm ghost-danger" type="submit">{{ locale.hk_borrar|raw }}</button>
            </form>
          </td>
        </tr>
      {% endfor %}
      </tbody>
    </table>
    {% endif %}
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
