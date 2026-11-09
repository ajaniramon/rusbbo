{% set title = locale.hk_filtro_de_palabras %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="toolbar">
  <input type="search" data-filter="words-table" placeholder="{{ locale.hk_buscar_palabra|raw }}">
  <span class="spacer"></span>
  <a class="btn primary" href="{{ hk.path }}/wordfilter/edit">{{ locale.hk_nueva_palabra|raw }}</a>
</div>

<div class="card flush">
  <div class="card-head"><h2>{{ locale.hk_palabras|raw }} <span class="sub">{{ words | length }}</span></h2><span class="muted small">{{ locale.hk_se_sustituyen_en_el_chat_por_el_texto_del_ajuste|raw }} <code>{{ locale.hk_wordfilter_word_replacement|raw }}</code>{{ locale.hk_los_cambios_llegan_al_juego_al_momento|raw }}</span></div>
  <div class="card-body scroll">
    {% if words is empty %}<div class="empty">{{ locale.hk_el_filtro_esta_vacio|raw }}</div>{% else %}
    <table class="tbl" id="words-table">
      <thead><tr><th>{{ locale.hk_palabra|raw }}</th><th>{{ locale.hk_filtrar_2|raw }}</th><th>{{ locale.hk_baneable|raw }}</th><th></th></tr></thead>
      <tbody>
      {% for word in words %}
        <tr>
          <td><a href="{{ hk.path }}/wordfilter/edit?id={{ word.id() }}"><strong>{{ word.word() }}</strong></a> <span class="muted small">#{{ word.id() }}</span></td>
          <td>{% if word.filterable() %}<span class="pill ok">{{ locale.hk_si|raw }}</span>{% else %}<span class="pill">{{ locale.hk_no|raw }}</span>{% endif %}</td>
          <td>{% if word.bannable() %}<span class="pill bad">{{ locale.hk_si|raw }}</span>{% else %}<span class="pill">{{ locale.hk_no|raw }}</span>{% endif %}</td>
          <td class="actions">
            <a class="btn sm" href="{{ hk.path }}/wordfilter/edit?id={{ word.id() }}">{{ locale.hk_editar|raw }}</a>
            <form class="inline" method="post" action="{{ hk.path }}/wordfilter/delete?id={{ word.id() }}" data-confirm="{{ locale.hk_quitar_3|raw }}{{ word.word() }}{{ locale.hk_del_filtro|raw }}">
              <input type="hidden" name="hk_token" value="{{ hk.token }}">
              <button class="btn sm ghost-danger" type="submit">{{ locale.hk_borrar|raw }}</button>
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
