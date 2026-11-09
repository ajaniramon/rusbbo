{% set title = (page != null) ? (locale.hk_editar_pagina ~ page.name()) : locale.hk_nueva_pagina_del_catalogo %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div><p>{% if page != null %}{{ locale.hk_pagina|raw }}{{ page.id() }}. {% endif %}{{ locale.hk_al_guardar_se_recarga_el_catalogo_de_todos_los_c|raw }}</p></div>
  <div class="actions">
    <a class="btn" href="{{ hk.path }}/catalogue/pages">{{ locale.hk_volver_a_las_paginas|raw }}</a>
    {% if page != null %}<a class="btn" href="{{ hk.path }}/catalogue/items?page={{ page.id() }}">{{ locale.hk_ver_sus_articulos|raw }}</a>{% endif %}
  </div>
</div>

<div class="card">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/catalogue/pages/edit{% if page != null %}?id={{ page.id() }}{% endif %}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <div class="row r-2">
        <div class="field"><label>{{ locale.hk_nombre|raw }}</label><input type="text" name="name" required value="{% if page != null %}{{ page.name() }}{% endif %}"></div>
        <div class="field">
          <label>{{ locale.hk_pagina_padre|raw }}</label>
          <select name="parent_id">
            <option value="-1" {% if page == null or page.parentId() == -1 %}selected{% endif %}>{{ locale.hk_raiz_del_catalogo|raw }}</option>
            {% for p in pages %}
              {% if page == null or p.id() != page.id() %}
              <option value="{{ p.id() }}" {% if page != null and page.parentId() == p.id() %}selected{% endif %}>{{ p.name() }} (#{{ p.id() }})</option>
              {% endif %}
            {% endfor %}
          </select>
        </div>
      </div>
      <div class="row">
        <div class="field"><label>{{ locale.hk_orden|raw }}</label><input type="number" name="order_id" value="{% if page != null %}{{ page.orderId() }}{% else %}1{% endif %}"></div>
        <div class="field">
          <label>{{ locale.hk_rango_minimo|raw }}</label>
          <select name="min_role">
            {% for rank in ranks %}
            <option value="{{ rank.getRankId() }}" {% if (page != null and page.minRole() == rank.getRankId()) or (page == null and rank.getRankId() == 1) %}selected{% endif %}>{{ rank.getRankId() }} · {{ hk.rankLabel(rank.getRankId()) }}</option>
            {% endfor %}
          </select>
        </div>
        <div class="field"><label>{{ locale.hk_layout|raw }}</label><input type="text" name="layout" class="mono" value="{% if page != null %}{{ page.layout() }}{% else %}ctlg_layout2{% endif %}"><span class="help">{{ locale.hk_p_ej_ctlg_layout2_ctlg_frontpage2_ctlg_pets|raw }}</span></div>
        <div class="field"><label>{{ locale.hk_id_antiguo|raw }}</label><input type="number" name="old_id" value="{% if page != null %}{{ page.oldId() }}{% else %}0{% endif %}"></div>
      </div>
      <div class="row">
        <div class="field"><label>{{ locale.hk_icono|raw }}</label><input type="number" name="icon" value="{% if page != null %}{{ page.icon() }}{% else %}0{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_color|raw }}</label><input type="number" name="colour" value="{% if page != null %}{{ page.colour() }}{% else %}0{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_inicio_de_temporada|raw }}</label><input type="text" name="seasonal_start" placeholder="{{ locale.hk_yyyy_mm_dd_hh_mm_ss|raw }}" value="{% if page != null and page.seasonalStart() != null %}{{ page.seasonalStart() }}{% endif %}"></div>
        <div class="field"><label>{{ locale.hk_duracion_de_temporada_s|raw }}</label><input type="number" name="seasonal_length" value="{% if page != null %}{{ page.seasonalLength() }}{% else %}0{% endif %}"></div>
      </div>
      <div class="form-actions">
        <label class="check"><input type="checkbox" name="is_navigatable" {% if page == null or page.navigatable() %}checked{% endif %}> {{ locale.hk_visible_en_el_indice_del_catalogo|raw }}</label>
        <label class="check"><input type="checkbox" name="is_club_only" {% if page != null and page.clubOnly() %}checked{% endif %}> {{ locale.hk_solo_socios_hc|raw }}</label>
      </div>
      <div class="row r-2">
        <div class="field"><label>{{ locale.hk_imagenes_lista_json|raw }}</label><textarea name="images" rows="4" class="mono">{% if page != null %}{{ page.images() }}{% else %}[]{% endif %}</textarea><span class="help">{{ locale.hk_ejemplo_catalog_frontpage_headline2_en_gif_ctlg|raw }}</span></div>
        <div class="field"><label>{{ locale.hk_textos_lista_json|raw }}</label><textarea name="texts" rows="4" class="mono">{% if page != null %}{{ page.texts() }}{% else %}[]{% endif %}</textarea><span class="help">{{ locale.hk_ejemplo_titulo_descripcion_de_la_pagina|raw }}</span></div>
      </div>
      <div class="form-actions"><button class="btn primary" type="submit">{% if page != null %}{{ locale.hk_guardar_pagina|raw }}{% else %}{{ locale.hk_crear_pagina|raw }}{% endif %}</button></div>
    </form>
  </div>
</div>

{% if page != null %}
<div class="card danger-zone mt">
  <div class="card-head"><h2>{{ locale.hk_zona_peligrosa|raw }}</h2></div>
  <div class="card-body form-actions">
    <form class="inline" method="post" action="{{ hk.path }}/catalogue/pages/delete?id={{ page.id() }}" data-confirm="{{ locale.hk_borrar_la_pagina|raw }}{{ page.name() }}{{ locale.hk_no_se_puede_deshacer_2|raw }}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <button class="btn danger" type="submit">{{ locale.hk_borrar_pagina|raw }}</button>
    </form>
    <span class="help">{{ locale.hk_los_articulos_asignados_a_esta_pagina_dejaran_de|raw }}</span>
  </div>
</div>
{% endif %}
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
